//! Send a key chord after a delay so you can focus a target window.
//! cargo run --example send_keys_test -- j l
use dial_win32::send_keys;

fn main() {
    let keys: Vec<String> = std::env::args().skip(1).collect();
    if keys.is_empty() {
        eprintln!("usage: send_keys_test <key> [key...]");
        return;
    }
    println!("sending {keys:?} in 3 seconds — focus the target window");
    std::thread::sleep(std::time::Duration::from_secs(3));
    let rejected = send_keys(&keys);
    println!("rejected: {rejected:?}");
}
