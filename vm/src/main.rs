#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unused_imports)]

mod vm;
mod sys;
mod asm;

extern crate sdl2;
use std::env;
use crate::vm::{VM, Value, MemBlock, ExitReason};
use crate::asm::{Assembler};

fn run_program(vm: &mut VM) -> Value
{
    use sdl2::event::Event;
    use sdl2::keyboard::Keycode;

    match vm.call(0, &[])
    {
        ExitReason::Exit(val) => {
            //dbg!(vm.stack_size());
            return val;
        }

        // Keep processig events
        ExitReason::Return(val) => {}
    }

    let mut event_pump = vm.sys_state.get_sdl_context().event_pump().unwrap();

    let mut i = 0;
    'main_loop: loop
    {
        // Process all pending events
        // See: https://docs.rs/sdl2/0.30.0/sdl2/event/enum.Event.html
        // TODO: we probably want to process window/input related events in window.rs ?
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    break 'main_loop
                },

                // TODO: we need to move mouse event handling to window.rs
                // tuck the SDL-specifics in there
                Event::MouseMotion { window_id, x, y, .. } => {
                    sys::window::window_call_mousemove(vm, window_id, x, y);
                }
                Event::MouseButtonDown { window_id, which, mouse_btn, .. } => {
                    sys::window::window_call_mousedown(vm, window_id, which, mouse_btn);
                }
                Event::MouseButtonUp { window_id, which, mouse_btn, .. } => {
                    sys::window::window_call_mouseup(vm, window_id, which, mouse_btn);
                }

                _ => {}
            }
        }

        let next_cb_time = sys::time::time_until_next_cb(&vm);

        if let Some(delay_ms) = next_cb_time {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        else
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // For each callback to run
        for pc in sys::time::get_cbs_to_run(vm)
        {
            match vm.call(pc, &[])
            {
                ExitReason::Exit(val) => {
                    return val;
                }
                ExitReason::Return(val) => {}
            }
        }
    }

    Value::from(0 as u32)
}

fn main()
{
    let args: Vec<String> = env::args().collect();
    //println!("{:?}", args);

    // TODO: command-line argument parsing
    // --allow <permissions>
    // --deny <permissions>
    // --allow-all

    if args.len() == 2 {
        let asm = Assembler::new();
        let mut vm = asm.parse_file(&args[1]).unwrap();
        let ret_val = run_program(&mut vm);
        std::process::exit(ret_val.as_i32());
    }

    std::process::exit(0);
}
