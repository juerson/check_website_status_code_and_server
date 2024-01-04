use chrono::Local;
use csv::Writer;
use ipnetwork::IpNetwork;
use lazy_static::lazy_static;
use rand::seq::SliceRandom;
use reqwest::{header::HeaderMap, Client};
use std::{
    error::Error,
    fmt,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    net::IpAddr,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::{
    sync::{mpsc, Semaphore, SemaphorePermit},
    time::{sleep, Duration},
};
use url::Url;

lazy_static! {
    // 存放server为cloudflare的IP的文件
    static ref IS_CLOUDFLARE_SERVER_FILE: Mutex<String> = Mutex::new("ip.txt".to_string());
}

//下面代码（一个struct、两个impl），主要用于处理无法在main函数中处理tcp_client_hello函数返回来的结果的问题
#[derive(Debug)]
struct CustomError(Box<dyn Error + Send>);

impl From<reqwest::Error> for CustomError {
    fn from(err: reqwest::Error) -> Self {
        CustomError(Box::new(err))
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 自定义错误消息的格式
        write!(f, "CustomError: {}", self.0)
    }
}

/* 读取文件的文件，并解析IP地址 */
fn read_and_parse_ips_from_file(filename: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let file = File::open(filename);
    let file_result = file.unwrap_or_else(|err| {
        // 处理文件不存在的错误
        eprintln!("打开{}文件，报错: {}", filename, err);
        wait_for_enter();
        std::process::exit(1); // 终止程序
    });
    let reader = BufReader::new(file_result);
    let mut unique_ips = std::collections::HashSet::new();

    for line in reader.lines() {
        if let Ok(ip_str) = line {
            let ip_type = determine_ip_type(&ip_str);
            if ip_type == "IPv4 CIDR" {
                let ips_from_cidr = generate_ipv4_ips_from_cidr(&ip_str)?;
                unique_ips.extend(ips_from_cidr);
            } else if ip_type == "IPv4" {
                unique_ips.insert(ip_str);
            } else if ip_type == "Domain Name" {
                unique_ips.insert(ip_str);
            } // IPv6 和 IPv6 CIDR 的省略
        }
    }

    let ips: Vec<String> = unique_ips.into_iter().collect();
    if ips.is_empty() {
        eprintln!("文件'{}'不能为空.", filename);
        wait_for_enter();
        std::process::exit(1);
    }

    Ok(ips)
}

/* 确定IP的类型（IPv4/IPv6、IPv4 CIDR、IPv6 CIDR、域名） */
fn determine_ip_type(address: &str) -> &str {
    if let Ok(ip_address) = IpAddr::from_str(address) {
        match ip_address {
            IpAddr::V4(_) => "IPv4",
            IpAddr::V6(_) => "IPv6",
        }
    } else if let Ok(ip_network) = address.parse::<IpNetwork>() {
        match ip_network {
            IpNetwork::V4(_) => "IPv4 CIDR",
            IpNetwork::V6(_) => "IPv6 CIDR",
        }
    } else {
        let address_to_parse =
            if !address.starts_with("http://") && !address.starts_with("https://") {
                format!("http://{}", address)
            } else {
                address.to_string()
            };

        if let Ok(url) = Url::parse(&address_to_parse) {
            if url.host_str().is_some() {
                "Domain Name"
            } else {
                ""
            }
        } else {
            ""
        }
    }
}

/* 生成IPv4地址 */
fn generate_ipv4_ips_from_cidr(cidr: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if let Ok(ip_network) = cidr.parse::<IpNetwork>() {
        let ips: Vec<String> = ip_network.iter().map(|ip| ip.to_string()).collect();
        Ok(ips)
    } else {
        Ok(Vec::new())
    }
}

/* 判断HTTP headers的信息(Server) */
fn get_server_header(headers: &HeaderMap) -> String {
    headers
        .get("Server")
        .map_or("", |s| s.to_str().unwrap_or(""))
        .to_lowercase()
}

/* 辅助函数 */
fn wait_for_enter() {
    print!("按Enter键退出程序>> ");
    io::stdout().flush().expect("Failed to flush stdout");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");
}

/* 获取当前时间 */
fn get_formatted_time() -> String {
    let current_time = Local::now();
    let formatted_time = current_time.format("%Y/%m/%d %H:%M:%S").to_string();
    formatted_time
}

/* 清空文件中的内容 */
fn clear_file_content() {
    let mut file = OpenOptions::new()
        .create(true) // 如果文件不存在，创建新文件
        .write(true) // 可写入文件
        .truncate(true) // 截断文件，即清空文件内容
        .open(IS_CLOUDFLARE_SERVER_FILE.lock().unwrap().as_str())
        .expect("Failed to open file for truncation");

    // 清空文件内容
    if let Err(err) = file.set_len(0) {
        eprintln!("清空文件内容失败: {:?}", err);
    }

    // 立即刷新文件
    if let Err(err) = file.flush() {
        eprintln!("刷新文件失败: {:?}", err);
    }
}

/* 写入txt文件中 */
fn write_to_file(ip: &str) -> Result<(), CustomError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(IS_CLOUDFLARE_SERVER_FILE.lock().unwrap().as_str())
        .map_err(|e| CustomError(Box::new(e)))?;

    writeln!(file, "{}", ip).map_err(|e| CustomError(Box::new(e)))?;

    // 立即刷新文件
    file.flush().map_err(|e| CustomError(Box::new(e)))?;

    Ok(())
}

// 异步地从一个信号量中获取一个许可证（permit），如果获取失败，就会产生一个panic
async fn acquire_semaphore(semaphore: &Arc<Semaphore>) -> SemaphorePermit {
    semaphore.acquire().await.expect("Semaphore acquire failed")
}

/* 获取请求头的server信息 */
async fn tcp_client_hello(
    ip: String,
    port: u16,
) -> Result<(String, u16, String, u16, String), CustomError> {
    const MAX_RETRIES: usize = 3;
    let mut retry_count = 0;
    let ip_type = determine_ip_type(ip.as_str());
    let url = format!(
        "http://{}{}",
        ip,
        if ip_type == "Domain Name" {
            "".to_owned()
        } else {
            format!(":{}", port)
        }
    );
    let print_address = if ip_type == "Domain Name" {
        ip.clone()
    } else {
        format!("{}:{}", ip, port)
    };
    let client = Client::new();

    loop {
        let start_time = Instant::now();
        match client
            .head(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => {
                let elapsed_time = start_time.elapsed().as_millis();
                let elapsed_time_str = format!("{:.2}", elapsed_time);
                // 获取server值和HTTP状态码
                let server_header = get_server_header(&response.headers());
                let status_code = response.status().as_u16();
                // 获取当前电脑的时间
                let formatted_time = get_formatted_time();
                println!(
                    "{} {} -> 响应时间：{} ms, 状态码：{}, Server：{}",
                    formatted_time, print_address, elapsed_time_str, status_code, server_header
                );

                return Ok((ip, port, elapsed_time_str, status_code, server_header));
            }
            Err(err) => {
                // 超时或远程服务器关闭连接，也有可能其他错误
                retry_count += 1;
                if retry_count >= MAX_RETRIES {
                    let formatted_time = get_formatted_time();
                    println!(
                        "{} {} -> 当前连接超时/其它错误，重试次数：已达到{}次上限！",
                        formatted_time, print_address, retry_count
                    );
                    return Err(err.into());
                } else {
                    let formatted_time = get_formatted_time();
                    println!(
                        "{} {} -> 当前连接超时/其它错误，重试次数：{} 次！",
                        formatted_time, print_address, retry_count
                    );
                }
            }
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 20)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();

    let filename = "ips-v4.txt";
    let mut ips = read_and_parse_ips_from_file(filename)?;
    // 清空IS_CLOUDFLARE_SERVER_FILE文件中原来的内容(存放server为cloudflare的IP的文件)
    clear_file_content();

    // 使用 SliceRandom trait 中的 shuffle 方法打乱向量
    let mut rng = rand::thread_rng();
    ips.shuffle(&mut rng);

    let ports = vec![443]; // 这里可以添加更多的端口
    let concurrent_limit = 500; // 限制并发的数量，可以适当修改这个数值

    let semaphore = Arc::new(Semaphore::new(concurrent_limit));
    // 创建通道，receiver用于接收任务结果
    let (sender, mut receiver) = mpsc::channel(ips.len() * ports.len());
    let mut handles = Vec::new();

    for ip in &ips {
        for port in &ports {
            let semaphore_permit = Arc::clone(&semaphore);
            let ip_clone = ip.clone();
            let port_clone = *port;
            let sender_clone = sender.clone();
            let handle = tokio::spawn(async move {
                let permit = acquire_semaphore(&semaphore_permit).await;
                let result = tcp_client_hello(ip_clone, port_clone).await;
                drop(permit);
                // 将任务结果发送到通道
                let send_result = sender_clone.send(result.map_err(|e| e.to_string())).await;
                // 用于处理"发送失败"
                if let Err(_err) = send_result {
                    // eprintln!("Failed to send result: {:?}", err);
                }
            });
            handles.push(handle);
        }
    }
    // 等待所有任务完成
    for handle in handles {
        let _ = handle.await;
    }

    /* 将结果写入文件中 */
    // 创建CSV写入器
    let mut writer = Writer::from_path("output.csv")?;
    // 首先写入CSV的标题
    writer.write_record(&["IP", "PORT", "Response Time(ms)", "Status Code", "Server"])?;

    // 存放IP地址和域名
    let mut ip_addresses: Vec<String> = Vec::new();
    let mut domain_names: Vec<String> = Vec::new();
    // 接收任务结果并处理
    while let Ok(result) = receiver.try_recv() {
        match result {
            Ok(response) => {
                let (ref ip, _port, ref _response_time, _status_code, ref server) = response;
                writer.serialize(&response)?;
                writer.flush()?;

                /* 下面将server为cloudflare的IP单独处理 */
                if server.to_lowercase() == "cloudflare" {
                    let ip_type = determine_ip_type(ip.as_str());
                    if ip_type == "Domain Name" {
                        domain_names.push(ip.clone());
                    } else {
                        ip_addresses.push(ip.clone());
                    }
                }
            }
            Err(_error) => {}
        }
    }

    // 合并两个向量，IP在前，域名在后
    ip_addresses.extend(domain_names);

    // 写入TXT文件（将server为cloudflare的IP地址添加到txt文件中，IP地址再前面，域名在后面）
    for ip in ip_addresses {
        if let Err(err) = write_to_file(&ip) {
            if let Some(file_path) = IS_CLOUDFLARE_SERVER_FILE.lock().ok() {
                eprintln!("写入'{}'文件失败: {:?}", file_path, err);
            } else {
                // eprintln!("Failed to acquire lock for IS_CLOUDFLARE_SERVER_FILE: {:?}", err);
            }
        }
    }

    println!("\n注意：如果扫描的目标是域名，则忽略端口。");
    println!(
        "任务执行完毕，耗时：{:?}；程序在3秒后自动退出！",
        start_time.elapsed()
    );
    sleep(tokio::time::Duration::from_secs(3)).await;
    Ok(())
}
