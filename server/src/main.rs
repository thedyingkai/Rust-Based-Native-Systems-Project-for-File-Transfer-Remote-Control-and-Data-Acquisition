use std::{
    fs::{File, OpenOptions, read_dir, create_dir_all},
    io::{self, BufReader, BufWriter, Write, Read, stdin, BufRead, copy},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf, Component},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const HOST: &str = "127.0.0.1";
const PORT: u16 = 9090;
const ROOT: &str = "data";

fn main() {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    // 处理控制台输入
    thread::spawn(move || {
        let stdin = stdin();
        for line in stdin.lines() {
            let line = line.expect("Failed to read line");
            match line.trim() {
                "quit" => {
                    println!("Shutting down server...");
                    running_clone.store(false, Ordering::SeqCst);
                    break;
                },
                _ => println!("Unknown command: {}", line.trim()),
            }
        }
    });
    
    // TCP 监听连接
    let listener = TcpListener::bind(format!("{}:{}", HOST, PORT)).expect("Failed to bind to address");
    listener.set_nonblocking(true).expect("Cannot set non-blocking");
    println!("Listening on {}:{}", HOST, PORT);

    // 确定 ROOT 目录存在
    if !Path::new(ROOT).exists() {
        create_dir_all(ROOT).expect("Failed to create root directory");
    }

    // 接受连接，为每个连接创建一个线程
    loop {
        if !running.load(Ordering::SeqCst) {
            println!("Server is shutting down...");
            break;
        }
        match listener.accept() {
            Ok((stream, _addr)) => {
                let root = ROOT.to_string();
                thread::spawn(move || handle_client_message(stream, &root));
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // 没有新连接，休眠一会再检查
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
                break;
            }
        }
    }
}

fn handle_client_message(stream: TcpStream, root: &str) {
    // 获取客户端地址
    let peer = stream.peer_addr().expect("Failed to get peer address");
    let mut reader = BufReader::new(stream.try_clone().expect("Failed to clone stream"));
    let mut writer = BufWriter::new(stream);

    loop {
        // 提取有效命令段
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            break;
        }
        let line = line.trim_end_matches(&['\r','\n'][..]).to_string();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let cmd = parts.next().unwrap_or("");

        match cmd {
            "quit" => {
                writeln!(writer, "Bye nya~").expect("Failed to write response");
                flush();
                break;
            }
            "list" => {
                let rel = parts.next().unwrap_or(".");
                let target = join_paths(root, rel, false);
                if !target.exists() || !target.is_dir() {
                    writeln!(writer, "{} not a dir", target).expect("Failed to write response");
                    flush();
                    continue;
                }
                writeln!(writer, "Here is the list nya:").expect("Failed to write response");
                match read_dir(&target) {
                    Ok(entries) => {
                        for entry in entries {
                            if let Ok(entry) = entry {
                                if let Ok(meta) = entry.metadata() {
                                    let name = entry.file_name().to_string_lossy().to_string();
                                    // 判断文件夹和文件
                                    if meta.is_dir() {
                                        writeln!(writer, "d 0 {}", name).expect("Failed to write response");
                                    } else if meta.is_file() {
                                        writeln!(writer, "f {} {}", meta.len(), name).expect("Failed to write response");
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        writeln!(writer, "ERR read dir failed").expect("Failed to write response");
                    }
                }
                flush();
            }
            "get" => {
                let rel = parts.next().unwrap_or("");
                let path = join_paths(root, rel, false);
                if !path.exists() || !path.is_file() {
                    writeln!(writer, "no such file called {}", path).expect("Failed to write response");
                    flush();
                    continue;
                }
                if let Ok(mut f) = File::open(&path) {
                    if let Ok(size) = f.metadata().map(|m| m.len()) {
                        writeln!(writer, "Get successful nya~").expect("Failed to write response");
                        flush();
                        copy(&mut f, &mut writer).expect("Failed to send file");
                        flush();
                    } else {
                        writeln!(writer, "ERR file error").expect("Failed to write response");
                        flush();
                    }
                } else {
                    writeln!(writer, "ERR file error").expect("Failed to write response");
                    flush();
                }
            }
            "put" => {
                let rel = parts.next().unwrap_or("");
                let size_part = parts.next().unwrap_or("0");
                let size: u64 = size_part.parse().unwrap_or(0);
                if size == 0 {
                    writeln!(writer, "ERR bad size").expect("Failed to write response");
                    flush();
                    continue;
                }
                let path = join_paths(root, rel, false);
                if let Some(parent) = path.parent() {
                    create_dir_all(parent).expect("Failed to create parent directory");
                }
                if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
                    let mut limited = reader.by_ref().take(size);
                    io::copy(&mut limited, &mut f).expect("Failed to write file");
                    writeln!(writer, "Update successful nya~").expect("Failed to write response");
                    flush();
                } else {
                    writeln!(writer, "ERR file error").ok();
                    flush();
                }
            }
            "help" => {
                write!(writer, "
Commands:\n\
  list [path] - List files in directory\n\
  get path - Get file content\n\
  put path size - Put file content (size in bytes)\n\
  quit - Exit the server\n\
                        ").ok();
                flush();
            }
            _ => {
                writeln!(writer, "ERR unknown cmd").expect("Failed to write response");
                flush();
            }
        }
        // 服务端日志
        eprintln!("[server] {:?} -> {}", peer, line);
    }
}

fn join_paths(base: &str, rel: &str, _for_create: bool) -> PathBuf {
    // 处理相对路径
    let mut p = PathBuf::from(base);
    let rel_path = Path::new(rel);
    for comp in rel_path.components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => continue,
            Component::ParentDir => { p.pop(); }
            Component::CurDir => {}
            Component::Normal(s) => p.push(s),
        }
    }
    p
}

fn flush() {
    writer.flush().expect("Failed to flush writer");
}