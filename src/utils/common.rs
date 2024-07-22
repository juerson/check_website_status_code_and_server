use chrono::Local;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
};

/* 将内容写入txt文件中 */
pub fn write_to_txt_file(content: String, output_file: &str) {
    let path: &Path = Path::new(output_file);
    let mut txt_writer_file: File = File::create(&path).expect("Failed to create file");
    txt_writer_file
        .write_all(content.as_bytes())
        .expect("Failed to write all data");
}

// 追写文件
pub fn append_or_create_and_write(content: &str, file_path: &str) -> std::io::Result<()> {
    // 检查文件是否存在
    let file_exists = Path::new(file_path).exists();

    // 打开文件，设置选项
    let mut file = match OpenOptions::new()
        .write(true)
        .append(file_exists)
        .create(!file_exists)
        .open(file_path)
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!("打开文件失败: {}", e);
            return Err(e);
        }
    };

    if let Err(e) = file.write_all(content.as_bytes()) {
        eprintln!("写入内容失败: {}", e);
        return Err(e);
    }
    Ok(())
}

/* 如果文件存在就删除文件 */
pub fn delete_if_file_exists(file_path: &str) -> std::io::Result<()> {
    let path = Path::new(file_path);

    if path.exists() {
        std::fs::remove_file(path)?;
    }

    Ok(())
}

/* 获取本地电脑的时间 */
pub fn get_current_time() -> String {
    let current_time = Local::now();
    let formatted_time: String = current_time.format("%Y/%m/%d %H:%M:%S").to_string();
    formatted_time
}

pub fn wait_for_enter() {
    print!("按Enter键退出程序>> ");
    io::stdout().flush().expect("Failed to flush stdout");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");
}
