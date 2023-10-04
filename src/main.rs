use std::fs::File;
use std::io::{BufRead, BufReader};
use std::io::{self, Write};
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Instant;
use reqwest;
use ipnetwork::IpNetwork;
use tokio::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};
use std::sync::Arc;
use tokio::sync::mpsc;
use csv::Writer;
use std::fmt;
use std::error::Error;

//下面代码（一个struct、两个impl），主要用于处理无法在main函数中打印tcp_client_hello函数返回来的结果的问题
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


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filename = "ip.txt";
    let ips = read_and_parse_ips_from_file(filename)?;

    let ports = vec![80, 8080, 443];
    let concurrent_limit = 100;
    let semaphore = Arc::new(Semaphore::new(concurrent_limit));
	
	let (sender, mut receiver) = mpsc::channel(ips.len() * ports.len());  // 创建通道，receiver用于接收任务结果
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
				// 如果要在main函数中，要接收结果并使用它们，就必须添加下面这个处理发送操作的的代码。
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
	
	println!("");
	
	// 创建CSV写入器
	let mut writer = Writer::from_path("output.csv")?;
	writer.write_record(&["IP", "PORT", "Response Time(ms)", "Server"])?; // 首先写入CSV的标题
	// 接收任务结果并处理
	while let Ok(result) = receiver.try_recv() {
		match result {
			Ok(response) => {
				
				// 打印处理成功的响应
				println!("Received response: {:?}", response);

				// 将结果写入CSV文件
				writer.serialize(&response)?;
			}
			Err(_error) => {
				// 处理错误信息
				// eprintln!("Error: {}", error);
			}
		}
	}

	// 关闭CSV写入器
	writer.flush()?;
	println!("");
	wait_for_enter(); // 等待用户按Enter键才退出窗口
    Ok(())
}

async fn acquire_semaphore(semaphore: &Arc<Semaphore>) -> SemaphorePermit {
    semaphore.acquire().await.expect("Semaphore acquire failed")
}

fn read_and_parse_ips_from_file(filename: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let file = File::open(filename);
	let file_result = file.unwrap_or_else(|err| {
        // 处理文件不存在的错误
        eprintln!("Error opening file: {}", err);
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
            } // IPv6 和 IPv6 CIDR 的省略
        }
    }

    let ips: Vec<String> = unique_ips.into_iter().collect();
    if ips.is_empty() {
        eprintln!("File '{}' is empty.", filename);
        std::process::exit(1);
    }

    Ok(ips)
}

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
        ""
    }
}

fn generate_ipv4_ips_from_cidr(cidr: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if let Ok(ip_network) = cidr.parse::<IpNetwork>() {
        let ips: Vec<String> = ip_network.iter().map(|ip| ip.to_string()).collect();
        Ok(ips)
    } else {
        Ok(Vec::new())
    }
}

async fn tcp_client_hello(ip: String, port: u16) -> Result<(String, u16, String, String), CustomError> {
    let start_time = Instant::now();
    let url = format!("http://{}:{}", ip, port);

    let client = reqwest::Client::new();
    let response = match client.head(&url).timeout(Duration::from_secs(5)).send().await {
        Ok(response) => response,
        Err(err) => {
            eprintln!("Error: timed out or connection closed"); // 超时或远程服务器关闭连接，也有可能其他错误
            return Err(err.into());
        }
    };

    let elapsed_time = start_time.elapsed().as_millis();
    let elapsed_time_str = format!("{:.2}", elapsed_time);
	
	// 获取响应头中的Server字段值，如果不存在则默认为空字符串
	let server_header = response.headers().get("Server").map_or("", |s| s.to_str().unwrap_or("")).to_lowercase();
	let status_code = response.status().as_u16();

	// 使用条件语句根据条件设置server变量的值
	let server = if !server_header.is_empty() {
		format!("{} {}",status_code,server_header)
	}  else {
		format!("{} No Server parameter",status_code)
	};

    println!("{}:{} {}ms {}", ip, port, elapsed_time_str, server);

    Ok((ip, port, elapsed_time_str, server))
}


fn wait_for_enter() {
    print!("Press Enter to exit...");
    io::stdout().flush().expect("Failed to flush stdout");

    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read line");
}
