mod utils;

use crate::utils::{
    common::{
        append_or_create_and_write, delete_if_file_exists, wait_for_enter, write_to_txt_file,
    },
    data::{determine_ipaddress_type, get_data_from_file},
    http_request::{acquire_semaphore, is_curl_installed, run_curl},
    locations::{check_and_download_location_file, load_location_file},
};
use csv::Writer;
use futures::future::join_all;
use rand::seq::SliceRandom;
use std::{fs::File, sync::Arc, time::Instant};
use tokio::sync::{mpsc, Semaphore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /* 涉及的相关文件 */
    let data_file: &str = "ips-v4.txt";
    let output_file: &str = "output.csv";
    let is_cloudflare_file: &str = "is_cloudflare.txt";
    let is_jetbrains_license_server_file = "is_jetbrains_license_server.txt";
    let location_file = "locations.json";
    let location_url = "https://speed.cloudflare.com/locations";

    // ——————————————————————— 检查curl工具是否安装；检查locations.json文件是否存在，不存在就下载 ———————————————————————

    // 检查电脑是否安装有curl，没有安装就退出程序
    if !is_curl_installed().await {
        println!("本电脑未安装curl命令工具");
        return Ok(());
    }

    // 下载locations.json文件
    check_and_download_location_file(location_file, location_url).await?;

    // ——————————————————————————————— 读取ips-v4.txt文件中的数据，并选择性生成IPv4地址 ————————————————————————————————

    let mut addresses: Vec<String> = get_data_from_file(data_file)?;

    // 使用 SliceRandom trait 中的 shuffle 方法打乱向量
    let mut rng = rand::thread_rng();
    addresses.shuffle(&mut rng);

    // 没有数据，就退出程序
    if addresses.len() == 0 {
        println!("没有读取到任何数据，请检查{}文件内容", data_file);
        wait_for_enter();
        std::process::exit(1);
    }

    let ports: Vec<u16> = vec![80]; // 这里可以修改为其它端口，或者添加更多的端口

    // ————————————————————————————————————————————— 并发执行run_curl函数 —————————————————————————————————————————————

    // 限制并发的数量
    let concurrent_limit: usize = 100;

    // 创建通道，receiver用于接收任务结果
    let (sender, mut receiver) = mpsc::channel(addresses.len() * ports.len());

    let semaphore: Arc<Semaphore> = Arc::new(Semaphore::new(concurrent_limit));
    let mut tasks = Vec::new();

    let data_center_locations: Vec<utils::locations::DataCenterLocations> =
        load_location_file(location_file)?;

    let start_time: Instant = Instant::now();

    for address in &addresses {
        for port in &ports {
            let semaphore_permit = Arc::clone(&semaphore);
            let address_clone: String = address.clone();
            let port_clone: u16 = *port;
            let sender_clone = sender.clone();
            let data_center_locations_clone = data_center_locations.clone();
            let task = tokio::spawn(async move {
                let permit = acquire_semaphore(&semaphore_permit).await;
                let result = run_curl(address_clone, port_clone, data_center_locations_clone).await;
                drop(permit);
                // 将任务结果发送到通道
                let send_result = sender_clone.send(result.map_err(|e| e.to_string())).await;
                // 用于处理"发送失败"
                if let Err(err) = send_result {
                    eprintln!("Failed to send result: {:?}", err);
                }
            });
            tasks.push(task);
        }
    }

    // 等待所有任务完成
    join_all(tasks).await;

    // 关闭发送通道
    drop(sender);

    // ———————————————————————————————————— 处理receiver结果，并将结果写入csv文件中 ————————————————————————————————————

    /* 将结果写入文件中 */
    let mut csv_writer_file: Writer<File> = Writer::from_path(output_file)?;

    // 首先写入CSV的标题
    csv_writer_file.write_record(&[
        "网络地址",
        "响应时间(ms)",
        "HTTP状态码",
        "数据中心",
        "国家代码",
        "服务器环境",
    ])?;

    // 存放IP地址、域名、是jetbrains激活服务器的地址
    let mut ip_addresses_vec: Vec<String> = Vec::new();
    let mut domain_addresses_vec: Vec<String> = Vec::new();
    let mut jetbrains_license_server_vec: Vec<String> = Vec::new();

    // 用于标记是否在最后写入说明字符串
    let mut flag = false;

    // 接收任务结果并处理
    while let Ok(result) = receiver.try_recv() {
        match result {
            Ok(response) => {
                let (
                    address,
                    port,
                    response_time,
                    http_status_code,
                    cf_ray_code,
                    country_code,
                    server_env,
                    jetbrains_license_server,
                ) = response;
                // 剔除不要的数据
                if http_status_code != 0 {
                    let ipaddress_type = determine_ipaddress_type(address.as_str());
                    let is_cloudflare = server_env.to_lowercase().contains("cloudflare");
                    let is_jetbrains_license =
                        jetbrains_license_server.to_lowercase().contains("true");
                    match ipaddress_type {
                        "Domain Name" => {
                            if is_cloudflare {
                                domain_addresses_vec.push(address.clone());
                            }

                            if is_jetbrains_license {
                                jetbrains_license_server_vec.push(address.clone());
                            }
                            // port = 443;
                        }
                        _ => {
                            if is_cloudflare {
                                ip_addresses_vec.push(address.clone());
                            }
                            if is_jetbrains_license {
                                let jetbrain_license_address = format!("{}:{}", address, port);
                                jetbrains_license_server_vec.push(jetbrain_license_address);
                            }
                        }
                    }
                    flag = true;
                    csv_writer_file.serialize([
                        address,
                        response_time,
                        http_status_code.to_string(),
                        cf_ray_code,
                        country_code,
                        server_env,
                    ])?;
                    csv_writer_file.flush()?;
                }
            }
            Err(_) => {}
        }
    }

    // 在后面插入一行，用于说明已经剔除无效数据（可以省略）
    if flag {
        csv_writer_file.serialize(["", "", "", "", "", "注意：已经剔除无效数据"])?;
        csv_writer_file.flush()?;
    }

    // —————————————————————— 分别将cloudflare和jetbrains_license_server相关的地址写入不同的txt文件中 ———————————————————

    // 合并两个向量，域名在前面，IP地址在后面
    domain_addresses_vec.extend(ip_addresses_vec);

    // 转换为字符串
    let cloudflare_content: String = domain_addresses_vec.join("\n");
    let license_server_content: String = jetbrains_license_server_vec.join("\n") + "\n"; // 结尾换行

    // 将Server为cloudflare的地址，写入txt文件中
    if !cloudflare_content.trim().is_empty() {
        write_to_txt_file(cloudflare_content, is_cloudflare_file);
    } else {
        delete_if_file_exists(is_cloudflare_file)?;
    }

    // 是Jetbrains的激活服务器的，追加写入txt文件中
    if !license_server_content.trim().is_empty() {
        append_or_create_and_write(&license_server_content, is_jetbrains_license_server_file)?;
    }

    // ———————————————————————————————————————————————————————————————————————————————————————————————————————————————

    println!("\n注意：如果扫描的目标是域名地址，则不需要添加端口。");
    println!("所有任务执行完毕，耗时：{:?}", start_time.elapsed());

    // 关闭接收通道
    drop(receiver);
    std::process::exit(0);
}
