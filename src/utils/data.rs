use crate::utils::common::wait_for_enter;
use ipnetwork::IpNetwork;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    net::IpAddr,
    str::FromStr,
};
use url::Url;

/* 读取文件的文件，并解析IP地址 */
pub fn get_data_from_file(file_path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let file = File::open(file_path);
    let file_result = file.unwrap_or_else(|err| {
        eprintln!("打开'{}'文件，报错: {}", file_path, err);
        wait_for_enter();
        std::process::exit(1); // 终止程序
    });
    let reader = BufReader::new(file_result);
    let mut unique_addresses = std::collections::HashSet::new();

    for line in reader.lines() {
        if let Ok(ipaddress_str) = line {
            let ipaddress_str = ipaddress_str.trim();
            if !ipaddress_str.is_empty() {
                let ipaddress_type = determine_ipaddress_type(&ipaddress_str);
                if ipaddress_type == "IPv4 CIDR" {
                    let ips_from_cidr = generate_ipv4_ips_from_cidr(&ipaddress_str)?;
                    unique_addresses.extend(ips_from_cidr);
                } else if ipaddress_type == "IPv4" {
                    unique_addresses.insert(ipaddress_str.to_string());
                } else if ipaddress_type == "Domain Name" {
                    unique_addresses.insert(ipaddress_str.to_string());
                } // IPv6 和 IPv6 CIDR 的省略
            }
        }
    }

    let addresses: Vec<String> = unique_addresses.into_iter().collect();
    if addresses.is_empty() {
        eprintln!("文件'{}'不能为空.", file_path);
        wait_for_enter();
        std::process::exit(1);
    }

    Ok(addresses)
}

/* 判断address的类型（IPv4/IPv6、IPv4 CIDR、IPv6 CIDR、域名） */
pub fn determine_ipaddress_type(address: &str) -> &str {
    if let Ok(ip_address) = IpAddr::from_str(address) {
        return match ip_address {
            IpAddr::V4(_) => "IPv4",
            IpAddr::V6(_) => "IPv6",
        };
    }

    if let Ok(ip_network) = address.parse::<IpNetwork>() {
        return match ip_network {
            IpNetwork::V4(_) => "IPv4 CIDR",
            IpNetwork::V6(_) => "IPv6 CIDR",
        };
    }

    let address_string = if address.starts_with("http://") || address.starts_with("https://") {
        address.to_string()
    } else {
        format!("http://{}", address)
    };

    if let Ok(url) = Url::parse(&address_string) {
        if url.host_str().is_some() {
            return "Domain Name";
        }
    }

    ""
}

/* 生成IPv4地址 */
pub fn generate_ipv4_ips_from_cidr(cidr: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if let Ok(ip_network) = cidr.parse::<IpNetwork>() {
        let ips: Vec<String> = ip_network.iter().map(|ip| ip.to_string()).collect();
        Ok(ips)
    } else {
        Ok(Vec::new())
    }
}
