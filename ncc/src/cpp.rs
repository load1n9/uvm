// Reference:
// https://gcc.gnu.org/onlinedocs/cpp/Preprocessor-Output.html

use std::path::Path;
use std::collections::HashMap;
use crate::parsing::*;

impl Input
{
    fn eat_spaces(&mut self)
    {
        loop
        {
            if self.eof() {
                break;
            }

            let ch = self.peek_ch();

            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.eat_ch();
            }
            else
            {
                break;
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Def
{
    name: String,
    params: Option<Vec<String>>,
    text: String,
}

/// Parse a definition or macro
fn parse_def(input: &mut Input) -> Result<Def, ParseError>
{
    let name = input.parse_ident()?;
    input.eat_spaces();

    let mut params = None;

    // If there are macro arguments
    if input.match_chars(&['(']) {
        let mut param_vec = Vec::default();

        loop
        {
            if input.match_token(")")? {
                break;
            }

            if input.eof() {
                return input.parse_error("eof inside define directive");
            }

            param_vec.push(input.parse_ident()?);

            if input.match_token(")")? {
                break;
            }

            input.expect_token(",")?;
        }

        params = Some(param_vec);
    }

    // Read text until \n
    let mut text = "".to_string();
    loop
    {
        if input.eof() {
            break;
        }

        let ch = input.peek_ch();

        if ch == '\n' {
            break;
        }

        // Backslash to keep reading on the next line
        if ch == '\\' {
            input.eat_ch();

            loop
            {
                if input.eof() {
                    break;
                }

                match input.eat_ch() {
                    '\n' => break,
                    '\r' => {},
                    ' ' => {},
                    _ => return input.parse_error("expected newline")
                }
            }
        }

        text.push(input.eat_ch());
    }

    text = text.trim().to_string();

    Ok(Def {
        name,
        params,
        text,
    })
}

fn process_ifndef(
    input: &mut Input,
    defs: &mut HashMap<String, Def>,
    gen_output: bool,
) -> Result<String, ParseError>
{
    let ident = input.parse_ident()?;
    let is_defined = defs.get(&ident).is_some();

    let mut output = String::new();

    // If not defined
    if !is_defined {
        // Process the then branch normally
        let mut end_keyword = "".to_string();
        output += &process_input_rec(
            input,
            defs,
            gen_output,
            &mut end_keyword
        )?;

        // If there is an else branch
        if end_keyword == "else" {
            let mut end_keyword = "".to_string();
            process_input_rec(
                input,
                defs,
                false,
                &mut end_keyword
            )?;

            if end_keyword != "endif" {
                return input.parse_error("expected #endif");
            }
        }
    }
    else
    {
        // Name defined, we need to ignore the then branch
        let mut end_keyword = "".to_string();
        process_input_rec(
            input,
            defs,
            false,
            &mut end_keyword
        )?;

        // If there is an else branch
        if end_keyword == "else" {
            let mut end_keyword = "".to_string();
            output += &process_input_rec(
                input,
                defs,
                gen_output,
                &mut end_keyword
            )?;

            if end_keyword != "endif" {
                return input.parse_error("expected #endif");
            }
        }
    }

    Ok(output)
}

// Read a macro argument
fn read_macro_arg(input: &mut Input, depth: usize) -> Result<String, ParseError>
{
    let mut output = "".to_string();

    loop
    {
        if input.eof() {
            return input.parse_error("end of input inside macro argument");
        }

        let ch = input.peek_ch();

        // If this is a character string
        if ch == '"' {
            output.push(input.eat_ch());
            loop
            {
                if input.eof() {
                    return input.parse_error("end of input inside string");
                }

                let ch = input.eat_ch();
                output.push(ch);

                if ch == '"' {
                    break;
                }

                if ch == '\\' {
                    let ch = input.eat_ch();
                    output.push(ch);
                    continue;
                }
            }

            continue;
        }

        // If this is an opening parenthesis
        if ch == '(' {
            input.eat_ch();
            output.push('(');
            output += &read_macro_arg(input, depth + 1)?;
            input.eat_ch();
            output.push(')');
            continue;
        }

        if ch == ')' {
            break;
        }

        if ch == ',' {
            if depth == 0 {
                break;
            }
        }

        output.push(input.eat_ch());
    }

    Ok(output)
}

/// Expand a definition or macro
fn expand_macro(
    input: &mut Input,
    defs: &mut HashMap<String, Def>,
    gen_output: bool,
    def: &Def,
) -> Result<String, ParseError>
{
    let mut text = def.text.clone();

    // If this is a macro with arguments
    if let Some(params) = &def.params {
        // If no arguments are provided,
        // don't expand the definition
        if !input.match_token("(")? {
            return Ok(def.name.clone());
        }

        let mut args = Vec::new();

        // For each macro argument
        loop
        {
            if input.eof() {
                return input.parse_error("unexpected end of input");
            }

            if input.match_token(")")? {
                break;
            }

            args.push(read_macro_arg(input, 0)?);

            if input.match_token(")")? {
                break;
            }

            input.expect_token(",")?;
        }

        // If the argument count doesn't match
        if args.len() != params.len() {
            return input.parse_error(&format!(
                "macro {} expected {} arguments",
                def.name,
                params.len()
            ));
        }

        // Replace the parameters by their value
        for (idx, param) in params.iter().enumerate() {
            text = text.replace(param, &args[idx]);
        }
    }

    // Process macros in text recursively
    let mut input = Input::new(&text, &input.src_name);
    let mut end_keyword = "".to_string();
    let sub_input = process_input_rec(
        &mut input,
        defs,
        gen_output,
        &mut end_keyword
    )?;

    if end_keyword != "" {
        return input.parse_error(&format!("unexpected #{}", end_keyword));
    }

    return Ok(sub_input);
}

/// Process the input and generate an output string
pub fn process_input(input: &mut Input) -> Result<String, ParseError>
{
    let mut defs = HashMap::new();

    let mut end_keyword = "".to_string();
    let result = process_input_rec(
        input,
        &mut defs,
        true,
        &mut end_keyword
    );

    if end_keyword != "" {
        return input.parse_error(&format!("unexpected #{}", end_keyword));
    }

    result
}

/// Process the input and generate an output string recursively
fn process_input_rec(
    input: &mut Input,
    defs: &mut HashMap<String, Def>,
    gen_output: bool,
    end_keyword: &mut String
) -> Result<String, ParseError>
{
    let mut output = String::new();

    // For each line of the input
    loop
    {
        if input.eof() {
            break;
        }

        let ch = input.peek_ch();

        // If this is a preprocessor directive
        if input.peek_ch() == '#' {
            input.eat_ch();
            let directive = input.parse_ident()?;
            input.eat_spaces();

            //println!("{}", directive);

            // If not defined
            if directive == "ifndef" {
                output += &process_ifndef(
                    input,
                    defs,
                    gen_output
                )?;
                continue
            }

            // On #else or #endif, stop
            if directive == "else" || directive == "endif" {
                *end_keyword = directive;
                break;
            }

            if gen_output && directive == "include" {
                let file_path = if input.peek_ch() == '<' {
                    let file_name = input.parse_str('>')?;
                    Path::new("include").join(file_name).display().to_string()
                }
                else
                {
                    input.parse_str('"')?
                };

                let mut input = Input::from_file(&file_path);

                let mut end_keyword = "".to_string();
                let include_output = process_input_rec(
                    &mut input,
                    defs,
                    gen_output,
                    &mut end_keyword
                )?;

                if end_keyword != "" {
                    return input.parse_error(&format!("unexpected #{}", end_keyword));
                }

                // TODO: emit linenum directive

                output += &include_output;

                // TODO: emit linenum directive

                continue;
            }

            // Definition or macro
            if gen_output && directive == "define" {
                let def = parse_def(input)?;
                defs.insert(def.name.clone(), def);
                continue
            }

            // Undefine a macro or constant
            if gen_output && directive == "undef" {
                let name = input.parse_ident()?;
                defs.remove(&name);
                continue
            }

            if gen_output {
                return input.parse_error(&format!(
                    "unknown preprocessor directive {}", directive
                ));
            }
        }

        // Eat single-line comments
        if input.match_chars(&['/', '/']) {
            input.eat_comment();
            // Do we want to copy over the content to the output to
            // avoid messing up the source position?
            continue;
        }

        // Eat multi-line comment
        if input.match_chars(&['/', '*']) {
            input.eat_multi_comment()?;
            // Do we want to copy over the content to the output to
            // avoid messing up the source position?
            continue;
        }

        // Keep track if we're inside of a string or not
        // We don't want to preprocess things inside strings
        if input.match_chars(&['"']) {
            output.push('"');
            loop
            {
                if input.eof() {
                    return input.parse_error("unexpected end of input inside string");
                }

                let ch = input.eat_ch();
                output.push(ch);

                if ch == '"' {
                    break;
                }

                if ch == '\\' {
                    let ch = input.eat_ch();
                    output.push(ch);
                    continue;
                }
            }

            continue;
        }

        // If this is an identifier
        if gen_output && is_ident_ch(ch) {
            let ident = input.parse_ident()?;

            // If we have a definition for this identifier
            if let Some(def) = defs.get(&ident) {
                let def = def.clone();
                output += &expand_macro(input, defs, gen_output, &def)?;
            }
            else if ident == "__LINE__" {
                output += &format!("{}", input.line_no);
            }
            else if ident == "__FILE__" {
                output += &format!("\"{}\"", input.src_name);
            }
            else
            {
                output += &ident;
            }

            continue;
        }

        output.push(input.eat_ch());
    }

    Ok(output)
}
