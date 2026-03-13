use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::Cli;

/// Application configuration, merged from file + CLI args
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Render delay for normal output (milliseconds)
    pub render_delay_ms: u64,

    /// Render delay for synchronized output blocks (milliseconds)
    pub sync_delay_ms: u64,

    /// Maximum lines in the history scrollback buffer
    pub history_lines: usize,

    /// Log level
    pub log_level: String,

    /// Log file path
    pub log_file: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            render_delay_ms: 5,
            sync_delay_ms: 50,
            history_lines: 100_000,
            log_level: "info".to_string(),
            log_file: None,
        }
    }
}

/// File-based config (optional, loaded from TOML)
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    pub render_delay_ms: Option<u64>,
    pub sync_delay_ms: Option<u64>,
    pub history_lines: Option<usize>,
    pub log_level: Option<String>,
    pub log_file: Option<String>,
}

impl AppConfig {
    /// Load configuration: file defaults < config file < CLI args
    pub fn load(cli: &Cli) -> Result<Self> {
        let mut config = AppConfig::default();

        // Try loading config file
        let config_path = cli.config.clone().unwrap_or_else(default_config_path);
        match std::fs::read_to_string(&config_path) {
            Ok(contents) => {
                let file_config: FileConfig = toml::from_str(&contents)
                    .with_context(|| format!("failed to parse config file: {config_path}"))?;
                info!(path = %config_path, "loaded config file");
                config.apply_file_config(&file_config);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %config_path, "no config file found, using defaults");
            }
            Err(e) => {
                warn!(path = %config_path, error = %e, "failed to read config file, using defaults");
            }
        }

        // CLI args override file config
        config.render_delay_ms = cli.render_delay_ms;
        config.sync_delay_ms = cli.sync_delay_ms;
        config.history_lines = cli.history_lines;
        config.log_level = cli.log_level.clone();
        if cli.log_file.is_some() {
            config.log_file = cli.log_file.clone();
        }

        Ok(config)
    }

    fn apply_file_config(&mut self, file: &FileConfig) {
        if let Some(v) = file.render_delay_ms {
            self.render_delay_ms = v;
        }
        if let Some(v) = file.sync_delay_ms {
            self.sync_delay_ms = v;
        }
        if let Some(v) = file.history_lines {
            self.history_lines = v;
        }
        if let Some(ref v) = file.log_level {
            self.log_level = v.clone();
        }
        if file.log_file.is_some() {
            self.log_file = file.log_file.clone();
        }
    }
}

fn default_config_path() -> String {
    dirs::config_dir()
        .map(|p| {
            p.join("claude-terminal")
                .join("config.toml")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| "config.toml".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_sane_values() {
        let config = AppConfig::default();
        assert_eq!(config.render_delay_ms, 5);
        assert_eq!(config.sync_delay_ms, 50);
        assert_eq!(config.history_lines, 100_000);
        assert_eq!(config.log_level, "info");
        assert!(config.log_file.is_none());
    }

    #[test]
    fn test_file_config_overrides_defaults() {
        let mut config = AppConfig::default();
        let file = FileConfig {
            render_delay_ms: Some(10),
            sync_delay_ms: Some(100),
            history_lines: Some(50_000),
            log_level: Some("debug".to_string()),
            log_file: Some("/tmp/test.log".to_string()),
        };
        config.apply_file_config(&file);
        assert_eq!(config.render_delay_ms, 10);
        assert_eq!(config.sync_delay_ms, 100);
        assert_eq!(config.history_lines, 50_000);
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.log_file, Some("/tmp/test.log".to_string()));
    }

    #[test]
    fn test_partial_file_config_preserves_defaults() {
        let mut config = AppConfig::default();
        let file = FileConfig {
            render_delay_ms: Some(10),
            ..Default::default()
        };
        config.apply_file_config(&file);
        assert_eq!(config.render_delay_ms, 10);
        assert_eq!(config.sync_delay_ms, 50); // default preserved
        assert_eq!(config.history_lines, 100_000); // default preserved
    }

    #[test]
    fn test_config_file_parsing() {
        let toml_str = r#"
render_delay_ms = 10
sync_delay_ms = 75
history_lines = 200000
log_level = "debug"
"#;
        let file_config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(file_config.render_delay_ms, Some(10));
        assert_eq!(file_config.sync_delay_ms, Some(75));
        assert_eq!(file_config.history_lines, Some(200_000));
        assert_eq!(file_config.log_level, Some("debug".to_string()));
        assert!(file_config.log_file.is_none());
    }
}
