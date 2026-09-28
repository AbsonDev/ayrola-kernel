use std::io::{BufRead, Write};

fn main() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.txt");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"{\"type\":\"message\"}\n\xFF\xFE\n{\"type\":\"message\"}\n")
        .unwrap();
    file.sync_all().unwrap();

    let file = std::fs::File::open(&path).unwrap();
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    let mut err_count = 0;
    for _ in 0..10 {
        match reader.read_line(&mut line) {
            Ok(n) => println!("Ok({n})"),
            Err(e) => {
                println!("Err({e})");
                err_count += 1;
            }
        }
    }
    assert!(err_count < 10, "infinite loop");
}
