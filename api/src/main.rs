#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_parens)]
#![allow(unused_mut)]

use std::fs;
use std::fs::File;
use std::io::Write;
use std::env;
use std::collections::HashSet;
use std::collections::HashMap;

// https://docs.rs/serde_json/latest/serde_json/
use serde::{Deserialize, Serialize};
use serde_json::Result;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SubSystem {
    subsystem: String,
    description: Option<String>,
    syscalls: Vec<SysCall>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SysCall {
    name: String,
    args: Vec<(String, String)>,
    returns: (String, String),
    permission: String,
    const_idx: Option<u16>,
    description: Option<String>,
}

impl SysCall
{
    fn c_sig_string(&self) -> String
    {
        let mut param_str = "".to_string();
        for (idx, arg) in self.args.iter().enumerate() {
            if idx > 0 {
                param_str += ", ";
            }
            param_str += &format!("{} {}", arg.0, arg.1);
        }

        format!("{} {}({})", self.returns.0, self.name, param_str)
    }
}

/// Verify that a string is a valid ascii identifier
fn is_valid_ident(name: &str) -> bool
{
    if name.len() == 0 {
        return false;
    }

    if name != name.to_lowercase() {
        return false;
    }

    let ch0 = name.chars().nth(0).unwrap();
    if ch0 != '_' && !ch0.is_ascii_alphabetic() {
        return false;
    }

    for ch in name.chars() {
        if ch != '_' && !ch.is_ascii_alphanumeric() {
            return false;
        }
    }

    return true;
}

/// Used to make sure that description strings all start with a capital
// letter and end with a period for consistency
fn normalize_description(text: &str) -> String
{
    let mut text = text.to_string().trim().to_string();

    if !text.ends_with(".") {
        text += ".";
    }

    let first_ch = text.chars().nth(0).unwrap();
    if first_ch.is_lowercase() {
        text = text.replacen(first_ch, &first_ch.to_uppercase().to_string(), 1);
    }

    text
}

fn main()
{
    let mut unique_names: HashSet<String> = HashSet::new();

    // Map from constant index to name
    let mut idx_to_name: Vec<Option<String>> = Vec::default();

    let syscalls_json = fs::read_to_string("syscalls.json").unwrap();
    let mut subsystems: Vec<SubSystem> = serde_json::from_str(&syscalls_json).unwrap();
    //println!("deserialized = {:?}", deserialized);

    // For each subsystem
    for subsystem in &mut subsystems {
        if !is_valid_ident(&subsystem.subsystem) {
            panic!();
        }

        if let Some(text) = &subsystem.description {
            subsystem.description = Some(normalize_description(&text));
        }

        // For each syscall for this subsystem
        for syscall in &mut subsystem.syscalls {
            // Make sure that syscall names are valid
            if !is_valid_ident(&syscall.name) {
                panic!();
            }

            if let Some(text) = &syscall.description {
                syscall.description = Some(normalize_description(&text));
            }

            // Make sure that syscall names are unique
            if unique_names.get(&syscall.name).is_some() {
                panic!("two syscalls have the name {}", syscall.name);
            }
            unique_names.insert(syscall.name.clone());

            // Fill the map of indices to names
            if let Some(const_idx) = syscall.const_idx {
                let const_idx = const_idx as usize;
                if const_idx >= idx_to_name.len() {
                    idx_to_name.resize(const_idx + 1, None);
                }

                if idx_to_name[const_idx].is_some() {
                    panic!("two syscalls have the same const_idx={}", const_idx);
                }

                idx_to_name[const_idx] = Some(syscall.name.clone());
            }
        }
    }

    // Verify that there are no gaps in the syscall indices,
    // that is, every syscall idx up to the maximun index is taken
    for (idx, maybe_name) in idx_to_name.iter().enumerate() {
        if maybe_name.is_none() {
            panic!();
        }
    }

    // Allocate new indices to the syscalls that don't have indices yet
    for mut subsystem in &mut subsystems {
        for syscall in &mut subsystem.syscalls {
            if syscall.const_idx.is_none() {
                let const_idx = idx_to_name.len() as u16;
                syscall.const_idx = Some(const_idx);
                idx_to_name.push(Some(syscall.name.clone()));
                println!("allocating const_idx={} to syscall \"{}\"", const_idx, syscall.name);
            }

        }
    }

    // Re-serialize the data and write it back to the JSON file
    let json_output = serde_json::to_string_pretty(&subsystems).unwrap();
    let mut file = File::create("syscalls.json").unwrap();
    file.write_all(json_output.as_bytes()).unwrap();

    gen_rust_bindings("../vm/src/sys/constants.rs", &subsystems);
    gen_c_bindings("../ncc/include/uvm/syscalls.h", &subsystems);
    gen_markdown("../doc/syscalls.md", &subsystems);
}

fn gen_rust_bindings(out_file: &str, subsystems: &Vec<SubSystem>)
{
    // Generate an array of syscalls sorted by const_idx
    let mut syscall_list: Vec<SysCall> = Vec::new();
    for subsystem in subsystems {
        for syscall in &subsystem.syscalls {
            syscall_list.push(syscall.clone());
        }
    }
    syscall_list.sort_by(|a, b| a.const_idx.unwrap().cmp(&b.const_idx.unwrap()));

    // Generate syscall constants in rust
    let mut file = File::create(out_file).unwrap();
    writeln!(&mut file, "//").unwrap();
    writeln!(&mut file, "// This file was automatically generated based on api/syscalls.json").unwrap();
    writeln!(&mut file, "//").unwrap();
    writeln!(&mut file).unwrap();

    writeln!(&mut file, "#![allow(unused)]").unwrap();
    writeln!(&mut file).unwrap();

    writeln!(&mut file, "pub const NUM_SYSCALLS: usize = {};", syscall_list.len()).unwrap();
    writeln!(&mut file).unwrap();

    // Constants for each syscall index
    for subsystem in subsystems {
        for syscall in &subsystem.syscalls {
            let name = syscall.name.to_uppercase();
            let idx = syscall.const_idx.unwrap();
            writeln!(&mut file, "pub const {}: u16 = {};", name, idx).unwrap();
        }
    }

    // Generate global array of syscall descriptors
    writeln!(&mut file).unwrap();
    writeln!(&mut file, "{}", concat!(
        "pub struct SysCallDesc\n",
        "{\n",
        "    pub name: &'static str,\n",
        "    pub const_idx: u16,\n",
        "    pub argc: usize,\n",
        "    pub has_ret: bool,\n",
        "}",
    )).unwrap();
    writeln!(&mut file).unwrap();

    writeln!(&mut file, "pub const SYSCALL_DESCS: [SysCallDesc; NUM_SYSCALLS] = [").unwrap();
    for syscall in syscall_list {
        let has_ret = syscall.returns.0 != "void";
        writeln!(
            &mut file,
            "    SysCallDesc {{ name: \"{}\", const_idx: {}, argc: {}, has_ret: {} }},",
            syscall.name,
            syscall.const_idx.unwrap(),
            syscall.args.len(),
            has_ret,
        ).unwrap();
    }
    writeln!(&mut file, "];").unwrap();
}

fn gen_c_bindings(out_file: &str, subsystems: &Vec<SubSystem>)
{
    // Generate C bindings
    let mut file = File::create(out_file).unwrap();
    writeln!(&mut file, "//").unwrap();
    writeln!(&mut file, "// This file was automatically generated based on api/syscalls.json").unwrap();
    writeln!(&mut file, "//").unwrap();
    writeln!(&mut file).unwrap();

    writeln!(&mut file, "#ifndef __UVM_SYSCALLS__").unwrap();
    writeln!(&mut file, "#define __UVM_SYSCALLS__").unwrap();
    writeln!(&mut file).unwrap();

    for subsystem in subsystems {
        for syscall in &subsystem.syscalls {
            let fn_name = syscall.name.clone();
            let c_sig_str = syscall.c_sig_string();

            let mut sys_arg_str = "".to_string();
            for (idx, arg) in syscall.args.iter().enumerate() {
                if idx > 0 {
                    sys_arg_str += ", ";
                }
                sys_arg_str += &format!("__{}", arg.1);
            }

            writeln!(&mut file, "// {}", c_sig_str).unwrap();

            // Add description comment if present
            if let Some(text) = &syscall.description {
                writeln!(&mut file, "// {}", text).unwrap();
            }

            writeln!(&mut file,
                "#define {}({}) asm ({}) -> {} {{ syscall {}; }}\n",
                fn_name,
                sys_arg_str,
                sys_arg_str,
                syscall.returns.0,
                fn_name,
            ).unwrap();

            /*
            let mut sys_arg_str = "".to_string();
            for (idx, arg) in syscall.args.iter().enumerate() {
                if idx > 0 {
                    sys_arg_str += ", ";
                }
                sys_arg_str += &arg.1;
            }

            writeln!(&mut file,
                "inline {}\n{{\n    return asm ({}) -> {} {{ syscall {}; }};\n}}\n",
                c_sig_str,
                sys_arg_str,
                syscall.returns.0,
                const_idx,
            ).unwrap();
            */
        }
    }

    writeln!(&mut file, "#endif").unwrap();
}

/// Generate markdown documentation
fn gen_markdown(out_file: &str, subsystems: &Vec<SubSystem>)
{
    let mut file = File::create(out_file).unwrap();

    writeln!(&mut file, "# UVM Subsystems and System Calls").unwrap();
    writeln!(&mut file).unwrap();
    writeln!(&mut file, "This file was automatically generated from [api/syscalls.json](/api/syscalls.json).").unwrap();
    writeln!(&mut file).unwrap();
    writeln!(&mut file, "The host APIs exposed to programs running on UVM are organized into").unwrap();
    writeln!(&mut file, "multiple subsystems described in this document.").unwrap();
    writeln!(&mut file, "Each subsystem includes a number of system calls (syscalls).").unwrap();
    writeln!(&mut file, "Arguments to syscalls are pushed on the stack in order.").unwrap();
    writeln!(&mut file, "Each syscall has fixed arity, that is, the number of input arguments is fixed,").unwrap();
    writeln!(&mut file, "and can output either 0 or 1 value on the stack.").unwrap();
    writeln!(&mut file, "The syscalls with a `void` return type do not output anything.").unwrap();
    writeln!(&mut file).unwrap();

    for subsystem in subsystems {
        writeln!(&mut file, "# {}", subsystem.subsystem).unwrap();
        writeln!(&mut file).unwrap();

        // Add description comment if present
        if let Some(text) = &subsystem.description {
            writeln!(&mut file, "{}", text).unwrap();
            writeln!(&mut file).unwrap();
        }

        for syscall in &subsystem.syscalls {
            writeln!(&mut file, "## {}", syscall.name).unwrap();
            writeln!(&mut file).unwrap();

            // C signature string
            writeln!(&mut file, "```\n{}\n```", syscall.c_sig_string()).unwrap();
            writeln!(&mut file).unwrap();

            // If this syscall returns something
            if syscall.returns.0 != "void" {
                writeln!(&mut file, "**Returns:** `{} {}`", syscall.returns.0, syscall.returns.1).unwrap();
                writeln!(&mut file).unwrap();
            }

            // Add description comment if present
            if let Some(text) = &syscall.description {
                writeln!(&mut file, "{}", text).unwrap();
                writeln!(&mut file).unwrap();
            }
        }
    }
}
