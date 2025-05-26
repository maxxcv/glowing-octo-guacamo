use std::{
  fs::{File, OpenOptions},
  io::{BufReader, BufWriter, Seek, SeekFrom, Write},
  path::Path,
  sync::Mutex,
  time::Instant,
};
use anyhow::Result;
use futures::StreamExt;
use lazy_static::lazy_static;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::{Deserialize, Serialize};
use tokio::{select, task};
use tokio_util::sync::CancellationToken;
use tauri::{command, AppHandle};

lazy_static! {
  static ref TOKENS: Mutex<std::collections::HashMap<String, CancellationToken>> =
    Mutex::new(std::collections::HashMap::new());
}

// 单个分片状态
#[derive(Serialize, Deserialize, Clone)]
struct SegmentState {
  start: u64,
  end: u64,
  downloaded: u64,
}

// 整体下载状态
#[derive(Serialize, Deserialize)]
struct DownloadState {
  id: String,
  url: String,
  output: String,
  total_size: u64,
  concurrency: usize,
  segments: Vec<SegmentState>,
}

impl DownloadState {
  fn state_file(&self) -> String {
    format!("{}.state", &self.output)
  }
  fn save(&self) -> Result<()> {
    let f = File::create(self.state_file())?;
    serde_json::to_writer(BufWriter::new(f), &self)?;
    Ok(())
  }
  fn load(output: &str) -> Option<Self> {
    let path = format!("{}.state", output);
    if Path::new(&path).exists() {
      let f = File::open(path).ok()?;
      serde_json::from_reader(BufReader::new(f)).ok()
    } else {
      None
    }
  }
}

#[derive(Serialize)]
struct ProgressPayload {
  download_id: String,
  transferred: u64,
  transfer_rate: f64,
  percentage: f64,
}

/// 开始或恢复下载
#[command]
async fn download(app: AppHandle, id: String, url: String, output: String) -> Result<(), String> {
  // 载入或初始化状态
  let mut state = DownloadState::load(&output).unwrap_or_else(|| {
    // HEAD 获取文件大小
    let rt = tokio::runtime::Runtime::new().unwrap();
    let total = rt.block_on(async {
      let h = reqwest::Client::new()
        .head(&url)
        .send().await.map_err(|e| e.to_string())?;
      Ok::<u64, String>(
        h.headers()
          .get(reqwest::header::CONTENT_LENGTH)
          .ok_or("无 Content-Length")?
          .to_str().unwrap()
          .parse().unwrap(),
      )
    }).unwrap();

    let concurrency = 8;
    let part = total / concurrency as u64;
    let mut segments = Vec::new();
    for i in 0..concurrency {
      let start = i as u64 * part;
      let end = if i==concurrency-1 { total-1 } else { (i as u64+1)*part -1 };
      segments.push(SegmentState { start, end, downloaded: 0 });
    }
    DownloadState { id: id.clone(), url: url.clone(), output: output.clone(), total_size: total, concurrency, segments }
  });

  // 生成取消 token 并存储
  let token = CancellationToken::new();
  TOKENS.lock().unwrap().insert(id.clone(), token.clone());

  // 重试策略
  let retry_mw = RetryTransientMiddleware::new_with_policy(
    ExponentialBackoff::builder().build_with_max_retries(3)
  );
  let client = reqwest::Client::builder()
    .with(retry_mw)
    .build().map_err(|e| e.to_string())?;

  // 进度推送
  let start_time = Instant::now();
  let mut last_emit = Instant::now();

  // 并发下载各分片
  let mut handles = vec![];
  for seg in state.segments.iter_mut() {
    let url = state.url.clone();
    let out = state.output.clone();
    let seg_copy = seg.clone();
    let client = client.clone();
    let cancel = token.clone();

    handles.push(task::spawn(async move {
      let mut downloaded = seg_copy.downloaded;
      let resp = client.get(&url)
        .header(reqwest::header::RANGE, format!("bytes={}-{}", seg_copy.start + downloaded, seg_copy.end))
        .send().await.map_err(|e| e.to_string())?;
      let mut stream = resp.bytes_stream();

      let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&out)
        .map_err(|e| e.to_string())?;
      f.seek(SeekFrom::Start(seg_copy.start + downloaded)).unwrap();

      while let Some(chunk) = select! {
        _ = cancel.cancelled() => break None,
        x = stream.next() => Some(x)
      } {
        let buf = chunk.map_err(|e| e.to_string())?;
        f.write_all(&buf).map_err(|e| e.to_string())?;
        downloaded += buf.len() as u64;
      }
      Ok::<u64, String>(downloaded)
    }));
  }

  // 监控并推送进度
  loop {
    if token.is_cancelled() {
      state.save().map_err(|e| e.to_string())?;
      return Err("Paused".into());
    }
    let mut total_dl = 0;
    for (i, h) in handles.iter_mut().enumerate() {
      if let Ok(Some(Ok(d))) = h.now_or_never() {
        state.segments[i].downloaded = d;
      }
      total_dl += state.segments[i].downloaded;
    }
    let pct = total_dl as f64 * 100.0 / state.total_size as f64;
    let elapsed = start_time.elapsed().as_secs_f64();
    let rate = total_dl as f64 / elapsed;

    if last_emit.elapsed().as_millis() >= 50 {
      let payload = ProgressPayload {
        download_id: id.clone(),
        transferred: total_dl,
        transfer_rate: rate,
        percentage: pct,
      };
      app.emit_all("DOWNLOAD_PROGRESS", payload).ok();
      last_emit = Instant::now();
    }
    if total_dl >= state.total_size {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
  }

  // 完成后删除 .state
  let _ = std::fs::remove_file(state.state_file());
  Ok(())
}

/// 暂停下载
#[command]
fn cancel_download(id: String) {
  if let Some(token) = TOKENS.lock().unwrap().remove(&id) {
    token.cancel();
  }
}
