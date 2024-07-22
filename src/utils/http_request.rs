use std::{collections::HashMap, process::Stdio, str, sync::Arc};
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::{timeout, Duration, Instant},
};

use crate::utils::{
    common::get_current_time,
    data::determine_ipaddress_type,
    error::CustomError,
    locations::{find_cca2, DataCenterLocations},
};

const MAX_RETRIES: usize = 3; // 最大重试次数（含第一次连接）
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5); // 设置单个url请求的超时时间
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15); // 设置单个url请求，总超时时间(包括3次重试)

/*  获取一个信号量，如果获取失败，就会产生一个panic */
pub async fn acquire_semaphore(semaphore: &Arc<Semaphore>) -> SemaphorePermit {
    semaphore.acquire().await.expect("Semaphore acquire failed")
}

/* 运行curl命令，获取响应时间，响应码，服务器环境信息，CF-RAY参数的值，Location参数的值，JetBrains License Server参数的值 */
pub async fn run_curl(
    ip: String,
    port: u16,
    data_center_locations: Vec<DataCenterLocations>,
) -> Result<(String, u16, String, u16, String, String, String, String), CustomError> {
    let ip_type = determine_ipaddress_type(&ip);
    let url = format!(
        "http://{}{}",
        ip,
        match ip_type {
            "Domain Name" => "".to_owned(),
            _ => format!(":{}", port),
        }
    );
    let print_address = if ip_type == "Domain Name" {
        ip.clone()
    } else {
        format!("{}:{}", ip, port)
    };

    let start_time = Instant::now();

    for retry_count in 0..MAX_RETRIES {
        let request_start_time = Instant::now();
        let result = timeout(
            REQUEST_TIMEOUT,
            tokio::process::Command::new("curl")
                .arg("-I")
                .arg(&url)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let elapsed_time = format!("{:.2}", request_start_time.elapsed().as_millis());
                let stdout = str::from_utf8(&output.stdout).unwrap_or("");
                let status_line = stdout.lines().next().unwrap_or("");
                let status_code = status_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("0")
                    .parse::<u16>()
                    .unwrap_or(0);

                // 从curl命令的输出中，获取需要的参数值
                let vec = get_parameters_from_curl(stdout);
                if vec.len() != 3 {
                    return Err(CustomError::UnexpectedError(
                        "Unexpected number of headers".to_string(),
                    ));
                }
                // 获取当前时间
                let formatted_time = get_current_time();

                // HTTP响应报头中，Server参数的值(服务器环境信息)
                let server_env = vec[0].clone();

                // HTTP响应报头中，CF-RAY参数的值
                let cf_ray = vec[1].clone();
                // ———— 提取出cf_ray中的一个部分字符，即location部分
                let mut parts = cf_ray.split('-');
                let _first_part = parts.next().unwrap_or("");
                let location: String = parts.next().unwrap_or("").to_string().to_uppercase();
                // ———— 使用location值来查询国家代码（即查找locations.json文件）
                let country_code = match find_cca2(data_center_locations, &location) {
                    Ok(cca2) => cca2.to_string(),
                    Err(_) => "".to_string(),
                };

                // HTTP响应报头中，Location参数的值(是否含account.jetbrains.com/fls-auth)
                let jetbrains_license_server = vec[2].clone();

                println!(
                    "{} {} -> Request successful, HTTP status code: {}, Response time: {}ms",
                    formatted_time, print_address, status_code, elapsed_time
                );

                return Ok((
                    ip,
                    port,
                    elapsed_time,
                    status_code,
                    location,
                    country_code,
                    server_env,
                    jetbrains_license_server,
                ));
            }
            Ok(Err(err)) => {
                let formatted_time = get_current_time();
                let retries_left = MAX_RETRIES - retry_count - 1;

                println!(
                    "{} {} -> Request failed, Requests remaining: {}",
                    formatted_time, print_address, retries_left
                );

                if retry_count >= MAX_RETRIES - 1 || start_time.elapsed() >= TOTAL_TIMEOUT {
                    return Err(CustomError::CommandExecutionFailed(err.to_string()));
                }
            }
            Err(_) => {
                let formatted_time = get_current_time();
                let retries_left = MAX_RETRIES - retry_count - 1;

                println!(
                    "{} {} -> Request timeout, Requests remaining: {}",
                    formatted_time, print_address, retries_left
                );

                if retry_count >= MAX_RETRIES - 1 || start_time.elapsed() >= TOTAL_TIMEOUT {
                    return Err(CustomError::CommandExecutionFailed("请求超时".to_string()));
                }
            }
        }
    }

    Err(CustomError::UnexpectedError(
        "Maximum retries exceeded".to_string(),
    ))
}

/* 获取当前时间 */
fn get_parameters_from_curl(headers: &str) -> Vec<String> {
    let mut header_map = HashMap::new();

    for line in headers.lines() {
        let line = line.to_lowercase();
        if line.starts_with("server:") {
            let value = line.splitn(2, ": ").nth(1).unwrap_or("");
            let first_part = value.splitn(2, ' ').nth(0).unwrap_or("").to_string();
            header_map.insert("server", first_part);
        } else if line.starts_with("cf-ray:") {
            header_map.insert(
                "cf-ray",
                line.splitn(2, ": ").nth(1).unwrap_or("").to_string(),
            );
        } else if line.starts_with("location:") {
            let value = line.splitn(2, ": ").nth(1).unwrap_or("");
            if value.contains("account.jetbrains.com/fls-auth") {
                header_map.insert("licenseServer", "true".to_string());
            } else {
                header_map.insert("licenseServer", "false".to_string());
            }
        }
    }

    let server = header_map.get("server").unwrap_or(&"".to_string()).clone();
    let cf_ray = header_map.get("cf-ray").unwrap_or(&"".to_string()).clone();
    let jetbrains_license_server = header_map
        .get("licenseServer")
        .unwrap_or(&"".to_string())
        .clone();

    vec![server, cf_ray, jetbrains_license_server]
}

/* 检查是否安装curl */
pub async fn is_curl_installed() -> bool {
    // 使用 `tokio::process::Command` 来异步运行命令
    let output = tokio::process::Command::new("curl")
        .arg("--version")
        .output()
        .await;

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}
