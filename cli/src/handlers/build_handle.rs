use reqwest::{multipart, Body};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use futures::StreamExt;
use shared::{config::MainConfig, docker::DockerService, err, ok};

use crate::{api::API, data::UserData, utils::get_unix_seconds};

pub struct BuildHandle {
    docker: DockerService,
    abs_path: PathBuf,
    remote_platform: Option<String>,
}

impl BuildHandle {
    pub fn new(
        docker: DockerService,
        abs_path: PathBuf,
        remote_platform: Option<String>,
    ) -> Result<Self> {
        ok!(Self {
            docker: docker,
            abs_path,
            remote_platform,
        })
    }
}
pub async fn build_images(bh: &BuildHandle, cfg: MainConfig, token: String) -> Result<()> {
    let build_tasks = get_build_tasks(cfg, bh.remote_platform.clone());
    for task in &build_tasks {
        print!("🔧 Building app: {}\n", task.app_name);
        let abs_context = bh.abs_path.join(&task.context);
        let mut stream = bh
            .docker
            .build_image(
                &task.docker_file_name,
                &task.tag,
                &abs_context.to_str().unwrap(),
                Some(&task.platform),
            )
            .await?;

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(msg) => print!("{}", msg.stream.unwrap_or("".to_string())),
                Err(err) => {
                    err!(anyhow!("⚠️  Error: {}", err))
                }
            }
        }
        println!("✔︎ Building Done: {}\n", task.app_name);
    }

    upload(bh, build_tasks, token).await?;
    ok!(())
}
pub async fn upload(bh: &BuildHandle, images: Vec<BuildTask>, token: String) -> Result<()> {
    let docker = Box::leak(Box::new(bh.docker.clone()));
    for task in images {
        print!("📤 Uploading image: {}\n", &task.app_name);

        let stream = docker.save_image(&task.tag).await;
        let stream_body = Body::wrap_stream(stream);

        let part = multipart::Part::stream(stream_body).file_name("image.tar");

        let form = multipart::Form::new().part("file", part);

        let remote_url = UserData::load_db(false)
            .await?
            .load_current_user()
            .await?
            .remote_url;
        API::new(&remote_url)?
            .upload_image(form, token.clone())
            .await?;
        println!("✔︎ Upload Done: {}\n", task.app_name);
    }

    ok!(())
}

#[derive(Clone)]
pub struct BuildTask {
    app_name: String,
    docker_file_name: String,
    context: PathBuf,
    tag: String,
    platform: String,
}

fn get_build_tasks(config: MainConfig, remote_platform: Option<String>) -> Vec<BuildTask> {
    let mut build_tasks = Vec::new();
    if let Some(apps) = config.app.as_ref() {
        for (app_name, app_config) in apps {
            build_tasks.push(BuildTask {
                app_name: app_name.clone(),
                docker_file_name: app_config
                    .dockerfile
                    .clone()
                    .unwrap_or("Dockerfile".to_string()),
                context: Path::new(&app_config.context.clone().unwrap_or("./".to_string()))
                    .to_path_buf(),
                tag: format!(
                    "{}-{}-image:{}",
                    config.project,
                    app_name,
                    get_unix_seconds().to_string()
                ),
                platform: remote_platform.clone().unwrap_or("linux/amd64".to_string()),
            })
        }
    }

    build_tasks
}
