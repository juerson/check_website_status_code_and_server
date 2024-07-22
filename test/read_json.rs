use serde::Deserialize;
use std::{error::Error, fs::File, io::Read};

// 数据中心的位置
#[derive(Debug, Clone, Deserialize)]
pub struct DataCenterLocations {
    iata: String,
    #[serde(rename = "lat")]
    _lat: f64,
    #[serde(rename = "lon")]
    _lon: f64,
    cca2: String,
    #[serde(rename = "region")]
    _region: String,
    #[serde(rename = "city")]
    _city: String,
}

fn find_cca2(
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

fn load_location_file(file_path: &str) -> Result<Vec<DataCenterLocations>, Box<dyn Error>> {
    let mut file = File::open(file_path)?;

    let mut json_data = String::new();
    file.read_to_string(&mut json_data)?;

    // 解析 JSON 数据
    let data_center_locations: Vec<DataCenterLocations> = serde_json::from_str(&json_data)?;
    Ok(data_center_locations)
}

fn main() -> Result<(), Box<dyn Error>> {
    let file_path = "locations.json";
    let target_iata = "IAD"; // 要查找的三字母代码

    let data_center_locations: Vec<DataCenterLocations> = load_location_file(file_path)?;
    let cca2 = match find_cca2(data_center_locations, target_iata) {
        Ok(cca2) => cca2.to_string(),
        Err(_) => "".to_string(),
    };
    println!("{} ——> {}", target_iata, cca2);

    Ok(())
}
