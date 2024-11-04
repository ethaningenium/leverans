pub mod deploy;

use std::collections::HashMap;

use anyhow::{anyhow, Result};

use crate::{
    config::{AppConfig, DbConfig, ServiceConfig},
    docker::{
        service::{ServiceMount, ServiceParam},
        DockerService,
    },
    err, ok,
    rollup::rollupables::EnvValues,
    SecretValue,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Deployable {
    // just name of the deployable, without the project name
    pub short_name: String,
    pub project_name: String,
    pub config_type: String,

    // host of the service in docker swarm as well
    pub service_name: String,
    pub docker_image: String,

    pub proxies: Vec<ProxyParams>,

    pub envs: HashMap<String, String>,
    pub volumes: HashMap<String, String>,
    pub mounts: HashMap<String, String>,
    pub args: Vec<String>,

    pub depends_on: Option<Vec<String>>,
    pub replicas: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyParams {
    pub port: u16,
    pub path_prefix: String,
    pub domain: String,
}

impl Deployable {
    pub fn from_app_config(
        name: String,
        config: AppConfig,
        project_name: String,
        image_tags: Vec<String>,
        secrets: Vec<SecretValue>,
        connectables: Vec<Connectable>,
    ) -> Result<Self> {
        // find right image
        let image_name = format!("{}-{}-image", project_name, name);
        let this_image_tags: Vec<_> = image_tags
            .into_iter()
            .filter(|i| i.starts_with(&image_name))
            .collect();

        // routing
        let proxy = if config.port.is_some() && config.domain.is_some() {
            Some(ProxyParams {
                port: config.port.unwrap(),
                path_prefix: config.path_prefix.unwrap_or("/".to_string()),
                domain: config.domain.unwrap(),
            })
        } else {
            None
        };

        ok!(Self {
            short_name: name.clone(),
            project_name: project_name.clone(),
            config_type: "app".to_string(),
            service_name: format!("{}-{}-service", project_name, name),
            docker_image: latest_tag(this_image_tags).ok_or(anyhow!("No images"))?,
            proxies: if proxy.is_some() {
                vec![proxy.unwrap()]
            } else {
                vec![]
            },
            envs: final_envs(config.envs, connectables, secrets),
            volumes: config.volumes.unwrap_or(HashMap::new()),
            mounts: config.mounts.unwrap_or(HashMap::new()),
            args: config.args.unwrap_or(vec![]),
            depends_on: None,
            replicas: 2,
        })
    }

    pub fn from_service_config(
        name: String,
        config: ServiceConfig,
        project_name: String,
        secrets: Vec<SecretValue>,
        connectables: Vec<Connectable>,
    ) -> Result<Self> {
        let proxy = if config.port.is_some() && config.domain.is_some() {
            Some(ProxyParams {
                port: config.port.unwrap(),
                path_prefix: config.path_prefix.unwrap_or("/".to_string()),
                domain: config.domain.unwrap(),
            })
        } else {
            None
        };

        ok!(Self {
            short_name: name.clone(),
            project_name: project_name.clone(),
            config_type: "service".to_string(),
            service_name: format!("{}-{}-service", project_name, name),
            docker_image: config.image,
            proxies: if proxy.is_some() {
                vec![proxy.unwrap()]
            } else {
                vec![]
            },
            envs: final_envs(config.envs, connectables, secrets),
            volumes: config.volumes.unwrap_or(HashMap::new()),
            mounts: config.mounts.unwrap_or(HashMap::new()),
            args: config.args.unwrap_or(vec![]),
            depends_on: None,
            replicas: 1,
        })
    }

    pub fn from_db_config(
        name: String,
        config: DbConfig,
        project_name: String,
        secrets: Vec<SecretValue>,
        connectables: Vec<Connectable>,
    ) -> Result<Self> {
        // Define default settings
        let mut envs;
        let mut volumes = HashMap::new();

        // Override default settings
        let image_name = match config.from.as_str() {
            "postgres" => {
                envs =
                    get_default_envs("postgres").ok_or(anyhow!("No default envs for postgres"))?;
                volumes.insert(
                    format!("{}-{}-volume", project_name, name),
                    "/var/lib/postgresql/data".to_string(),
                );
                "postgres".to_string()
            }
            "mysql" => {
                envs = get_default_envs("mysql").ok_or(anyhow!("No default envs for mysql"))?;
                volumes.insert(
                    format!("{}-{}-volume", project_name, name),
                    "/var/lib/mysql".to_string(),
                );
                "mysql".to_string()
            }
            _ => {
                err!(anyhow!("Invalid database type"))
            }
        };
        let user_envs = final_envs(config.envs, connectables, secrets);
        for (k, v) in user_envs {
            envs.insert(k, v);
        }

        ok!(Self {
            short_name: name.clone(),
            project_name: project_name.clone(),
            config_type: "db".to_string(),
            service_name: format!("{}-{}-service", project_name, name),
            docker_image: image_name,
            proxies: vec![],
            envs,
            volumes: volumes,
            mounts: config.mounts.unwrap_or(HashMap::new()),
            args: config.args.unwrap_or(vec![]),
            depends_on: None,
            replicas: 1
        })
    }

    pub async fn deploy(
        &self,
        docker: DockerService,
        service_names: Vec<String>,
        network_name: String,
    ) -> Result<()> {
        if service_names.contains(&self.service_name) {
            docker
                .update_service(self.to_docker_params(network_name)?)
                .await?;
            ok!(())
        } else {
            docker
                .create_service(self.to_docker_params(network_name)?)
                .await?;
            ok!(())
        }
    }

    pub fn to_docker_params(&self, network_name: String) -> Result<ServiceParam> {
        let mut service_mounts = vec![];

        for (k, v) in &self.volumes {
            service_mounts.push(ServiceMount::Volume(k.clone(), v.clone()));
        }

        for (k, v) in &self.mounts {
            service_mounts.push(ServiceMount::Bind(k.clone(), v.clone()));
        }
        ok!(ServiceParam {
            name: self.service_name.clone(),
            image: self.docker_image.clone(),
            network_name: network_name,
            labels: self.get_labels(),
            exposed_ports: HashMap::new(),
            envs: self.envs.clone(),
            mounts: service_mounts,
            args: self.args.clone(),
            cpu: 1.0,
            memory: 1024,
            replicas: self.replicas.try_into()?,
            constraints: vec![],
        })
    }

    pub fn get_labels(&self) -> HashMap<String, String> {
        let mut labels = HashMap::new();
        labels.insert("traefik.enable".into(), "true".into());
        let first_proxy = &self
            .proxies
            .get(0)
            .expect("We expected at least one proxy settings");
        let domain = &first_proxy.domain;
        let path_prefix = &first_proxy.path_prefix;
        let host = &self.service_name;
        let port = &first_proxy.port;
        let mut host_params = format!("Host(`{}`)", domain.clone());
        if path_prefix != "/" {
            host_params.push_str(format!(" && PathPrefix(`{}`)", path_prefix.clone()).as_str());
        }
        labels.insert(
            format!("traefik.http.routers.{}.rule", host.clone()),
            host_params,
        );
        labels.insert(
            format!("traefik.http.routers.{}.service", host.clone()),
            host.clone(),
        );
        labels.insert(
            format!(
                "traefik.http.services.{}.loadbalancer.server.port",
                host.clone()
            ),
            port.to_string(),
        );
        labels.insert(
            format!("traefik.http.routers.{}.tls", host.clone()),
            "true".into(),
        );
        labels.insert(
            format!("traefik.http.routers.{}.entrypoints", host.clone()),
            "websecure".into(),
        );
        labels
    }
}

pub fn get_default_envs(service: &str) -> Option<HashMap<String, String>> {
    match service {
        "mysql" => {
            let mut envs = HashMap::new();
            envs.insert("MYSQL_DATABASE".to_string(), "mydb".to_string());
            envs.insert("MYSQL_USER".to_string(), "myuser".to_string());
            envs.insert("MYSQL_PASSWORD".to_string(), "mypassword".to_string());
            envs.insert(
                "MYSQL_ROOT_PASSWORD".to_string(),
                "myrootpassword".to_string(),
            );
            Some(envs)
        }
        "postgres" => {
            let mut envs = HashMap::new();
            envs.insert("POSTGRES_DB".to_string(), "mydb".to_string());
            envs.insert("POSTGRES_USER".to_string(), "mypguser".to_string());
            envs.insert("POSTGRES_PASSWORD".to_string(), "mypassword".to_string());
            Some(envs)
        }
        _ => None,
    }
}

pub fn final_envs(
    envs: Option<HashMap<String, String>>,
    connectables: Vec<Connectable>,
    secrets: Vec<SecretValue>,
) -> HashMap<String, String> {
    let final_envs: HashMap<String, String> = envs
        .unwrap()
        .into_iter()
        .map(|(key, value)| (key, EnvValues::parse_env(&value).ok()))
        .filter(|(_, v)| v.is_some())
        .map(|(k, v)| (k, v.unwrap()))
        .map(|(k, v)| {
            let final_value = match v {
                EnvValues::This { service, method } => {
                    let connectable = connectables.iter().find(|c| c.short_name == service);
                    match method.as_str() {
                        "connection" | "conn" => {
                            if let Some(connectable) = connectable {
                                connectable.connection.clone().unwrap_or("".to_string())
                            } else {
                                "".to_string()
                            }
                        }
                        "internal" | "link" | "url" => {
                            if let Some(connectable) = connectable {
                                connectable.internal_link.clone().unwrap_or("".to_string())
                            } else {
                                "".to_string()
                            }
                        }
                        _ => "".to_string(),
                    }
                }
                EnvValues::Text(text) => text,
                EnvValues::Secret(secret_key) => secrets
                    .iter()
                    .find(|s| s.key == secret_key)
                    .map(|s| s.value.clone())
                    .unwrap_or("".to_string()),
            };

            (k, final_value)
        })
        .collect();

    final_envs
}

fn latest_tag(app_images: Vec<String>) -> Option<String> {
    let mut image_longest_time: u64 = 0;
    let mut last_image_name: Option<String> = None;
    for image in app_images {
        let parts: Vec<&str> = image.splitn(2, ":").collect();
        if parts.is_empty() || parts.len() != 2 {
            continue;
        }
        let image_time: u64 = parts[1].parse().unwrap_or(0);
        if image_time > image_longest_time {
            image_longest_time = image_time;
            last_image_name = Some(parts.join(":"));
        }
    }

    last_image_name
}

#[derive(Debug, Clone)]
pub struct Connectable {
    pub short_name: String,
    pub project_name: String,

    pub connection: Option<String>,
    pub internal_link: Option<String>,
}

impl Connectable {
    pub fn from_service_config(
        name: String,
        config: ServiceConfig,
        project_name: String,
    ) -> Result<Self> {
        let connection = None;
        let mut internal_link = None;
        if config.domain.is_some() && config.port.is_some() {
            internal_link = Some(format!(
                "{}-{}-service:{}",
                project_name,
                name,
                config.port.unwrap()
            ));
        }
        ok!(Self {
            short_name: name.clone(),
            project_name: project_name.clone(),
            connection,
            internal_link
        })
    }

    pub fn from_db_config(name: String, config: DbConfig, project_name: String) -> Result<Self> {
        let connection = match config.from.as_str() {
            "postgres" => {
                let default_envs =
                    get_default_envs("postgres").ok_or(anyhow!("No default envs for postgres"))?;
                let user_envs = config.envs.unwrap_or(HashMap::new());
                let username = user_envs
                    .get("POSTGRES_USER")
                    .unwrap_or(&default_envs["POSTGRES_USER"]);
                let password = user_envs
                    .get("POSTGRES_PASSWORD")
                    .unwrap_or(&default_envs["POSTGRES_PASSWORD"]);
                let dbname = user_envs
                    .get("POSTGRES_DB")
                    .unwrap_or(&default_envs["POSTGRES_DB"]);
                Some(format!(
                    "postgres://{}:{}@{}:5432/{}",
                    username,
                    password,
                    format!("{}-{}-service", project_name, name),
                    dbname
                ))
            }
            "mysql" => {
                let default_envs =
                    get_default_envs("mysql").ok_or(anyhow!("No default envs for mysql"))?;
                let user_envs = config.envs.unwrap_or(HashMap::new());
                let username = user_envs
                    .get("MYSQL_USER")
                    .unwrap_or(&default_envs["MYSQL_USER"]);
                let password = user_envs
                    .get("MYSQL_PASSWORD")
                    .unwrap_or(&default_envs["MYSQL_PASSWORD"]);
                let dbname = user_envs
                    .get("MYSQL_DATABASE")
                    .unwrap_or(&default_envs["MYSQL_DATABASE"]);
                Some(format!(
                    "mysql://{}:{}@{}:3306/{}",
                    username,
                    password,
                    format!("{}-{}-service", project_name, name),
                    dbname
                ))
            }
            _ => {
                err!(anyhow!("Invalid database type"))
            }
        };
        let internal_link = None;
        ok!(Self {
            short_name: name.clone(),
            project_name: project_name.clone(),
            connection,
            internal_link
        })
    }

    pub fn from_app_config(name: String, config: AppConfig, project_name: String) -> Result<Self> {
        let connection = None;
        let mut internal_link = None;
        if config.domain.is_some() && config.port.is_some() {
            internal_link = Some(format!(
                "{}-{}-service:{}",
                project_name,
                name,
                config.port.unwrap()
            ));
        }
        ok!(Self {
            short_name: name.clone(),
            project_name: project_name.clone(),
            connection,
            internal_link
        })
    }
}