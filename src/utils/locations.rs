use reqwest::Client;
use serde::Deserialize;
use std::{error::Error, io::Read, path::Path};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Deserialize)]
pub struct DataCenterLocations {
    #[serde(rename = "iata")]
    iata: String,
    #[serde(rename = "lat")]
    _lat: f64,
    #[serde(rename = "lon")]
    _lon: f64,
    #[serde(rename = "cca2")]
    cca2: String,
    #[serde(rename = "region")]
    _region: String,
    #[serde(rename = "city")]
    _city: String,
}

/* 读取locations.json文件，并解析 JSON 数据 */
pub fn load_location_file(file_path: &str) -> Result<Vec<DataCenterLocations>, Box<dyn Error>> {
    let mut file = std::fs::File::open(file_path)?;

    let mut json_data = String::new();
    file.read_to_string(&mut json_data)?;

    // 解析 JSON 数据
    let data_center_locations: Vec<DataCenterLocations> = serde_json::from_str(&json_data)?;
    Ok(data_center_locations)
}

pub fn find_cca2(
    data_center_locations: Vec<DataCenterLocations>,
    target_iata: &str,
) -> Result<String, Box<dyn Error>> {
    // 查找匹配的 iata，并返回 cca2
    for data_center_location in data_center_locations {
        if data_center_location.iata == target_iata {
            return Ok(data_center_location.cca2);
        }
    }
    // 没有找到就返回空字符串
    Err("".into())
}

/* 如果文件不存在，则从网上下载 */
pub async fn check_and_download_location_file(
    file_path: &str,
    url: &str,
) -> Result<(), Box<dyn Error>> {
    if !Path::new(file_path).exists() {
        println!("{} 文件不存在。准备从网上下载...", file_path);

        // 创建一个HTTP客户端
        let client = Client::new();

        // 异步下载
        let response = client.get(url).send().await?;

        if response.status().is_success() {
            // 异步读取响应文本
            let content = response.text().await?;

            // 将内容写入文件
            let mut file = tokio::fs::File::create(file_path).await?;
            file.write_all(content.as_bytes()).await?;

            println!("文件下载并保存为 {}\n", file_path);
        } else {
            eprintln!("下载文件失败：HTTP {}\n", response.status());
        }
    } else {
        println!("{} 文件已经存在。\n", file_path);
    }

    Ok(())
}
