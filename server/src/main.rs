use std::io::BufRead;

const HOST: &str = "127.0.0.1";
const PORT: u16 = 9090;
const ROOT: &str = "data";

fn handle_client_message(stream: std::net::TcpStream, root: &str) {
    let mut reader = std::io::BufReader::new(stream.try_clone().expect("Failed to clone stream"));
    let mut writer = std::io::BufWriter::new(stream);

    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("Failed to read line");
        let line = line.trim_end_matches(&['\r','\n'][..]).to_string();
        let mut parts = line.splitn(3, ' ');
        let cmd = parts.next().unwrap_or("").to_uppercase();

        println!("Received command: {}", cmd);
    }
}

fn main() {
    let mut running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_clone = running.clone();

    // 处理控制台输入
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = line.expect("Failed to read line");
            match line.trim() {
                "quit" => {
                    println!("Shutting down server...");
                    running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
                    break;
                },
                _ => println!("Unknown command: {}", line.trim()),
            }
        }
    });
    
    // TCP 监听连接
    let listener = std::net::TcpListener::bind(format!("{}:{}", HOST, PORT)).expect("Failed to bind to address");
    listener.set_nonblocking(true).expect("Cannot set non-blocking");
    println!("Listening on {}:{}", HOST, PORT);

    // 确定 ROOT 目录存在
    if !std::path::Path::new(ROOT).exists() {
        std::fs::create_dir_all(ROOT).expect("Failed to create root directory");
    }

    // 接受连接，为每个连接创建一个线程
    loop {
        if !running.load(std::sync::atomic::Ordering::SeqCst) {
            println!("Server is shutting down...");
            break;
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                let root = ROOT.to_string();
                std::thread::spawn(move || handle_client_message(stream, &root));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 没有新连接，休眠一会再检查
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
                break;
            }
        }
    }
}
