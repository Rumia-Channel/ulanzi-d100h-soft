//! Send a key chord or wheel notches after a delay so you can focus (or, for
//! the wheel, hover) a target window.
//! cargo run --example send_keys_test -- ctrl +
//! cargo run --example send_keys_test -- --wheel -2 alt     (--hwheel: horizontal)
#[cfg(target_os = "linux")]
use dial_linux::{send_keys, send_wheel};
#[cfg(windows)]
use dial_win32::{send_keys, send_wheel};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: send_keys_test <key> [key...] | --wheel|--hwheel <notches> [modifier...]");
        return;
    }
    #[cfg(target_os = "linux")]
    println!("backend: {:?}", dial_linux::init_input());
    println!("sending {args:?} in 3 seconds — focus the target window");
    std::thread::sleep(std::time::Duration::from_secs(3));
    let result = match args[0].as_str() {
        flag @ ("--wheel" | "--hwheel") => match args.get(1).and_then(|n| n.parse().ok()) {
            Some(notches) => send_wheel(&args[2..], notches, flag == "--hwheel"),
            None => {
                eprintln!("{flag} needs a notch count");
                return;
            }
        },
        _ => send_keys(&args),
    };
    println!("result: {result:?}");
}
