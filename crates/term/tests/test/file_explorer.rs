use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use term::application::Application;
use ui_core::input::parse_macro;

use super::helpers::{run_event_loop_until_idle, test_config, AppBuilder};

struct Fixture {
    app: Application,
    _dir: tempfile::TempDir,
    root: PathBuf,
    file: PathBuf,
}

impl Fixture {
    fn new(name: &str, content: &str) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().to_owned();
        let file = root.join(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, content)?;
        let app = AppBuilder::new()
            .with_config(test_config())
            .with_file(&file, None)
            .build()?;
        Ok(Self {
            app,
            _dir: dir,
            root,
            file,
        })
    }

    async fn keys(&mut self, keys: &str) -> anyhow::Result<()> {
        for key in parse_macro(keys)? {
            #[cfg(not(windows))]
            let event = termina::event::Event::Key(key.into());
            #[cfg(windows)]
            let event = crossterm::event::Event::Key(key.into());
            self.app.handle_terminal_events(Ok(event)).await;
        }
        self.app.editor.reset_idle_timer();
        tokio::time::timeout(
            Duration::from_secs(10),
            run_event_loop_until_idle(&mut self.app),
        )
        .await?;
        Ok(())
    }

    async fn open_explorer(&mut self) -> anyhow::Result<()> {
        self.keys("<space>E").await
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    fn status(&self) -> String {
        self.app
            .editor
            .get_status()
            .map(|(status, _)| status.to_string())
            .unwrap_or_default()
    }

    async fn finish(&mut self) {
        assert!(self.app.close().await.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn creates_files_and_directories_from_the_explorer() -> anyhow::Result<()> {
    let mut f = Fixture::new("alpha.txt", "alpha")?;
    f.open_explorer().await?;
    f.keys("alpha").await?;
    f.keys("<A-n>new.txt<ret>").await?;
    assert!(f.path("new.txt").is_file());
    assert_eq!(std::fs::read_to_string(f.path("new.txt"))?, "");

    f.keys("<A-n>newdir/<ret>").await?;
    assert!(f.path("newdir").is_dir());
    f.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn renames_and_moves_the_selected_entry() -> anyhow::Result<()> {
    let mut f = Fixture::new("alpha.txt", "alpha")?;
    std::fs::create_dir(f.path("sub"))?;
    f.open_explorer().await?;

    f.keys("alpha").await?;
    f.keys("<A-r><C-a><C-k>beta.txt<ret>").await?;
    assert!(!f.path("alpha.txt").exists());
    assert_eq!(std::fs::read_to_string(f.path("beta.txt"))?, "alpha");

    f.keys("<esc><space>E").await?;
    f.keys("beta").await?;
    f.keys("<A-m><C-a><C-k>sub<ret>").await?;
    assert!(!f.path("beta.txt").exists());
    assert_eq!(std::fs::read_to_string(f.path("sub/beta.txt"))?, "alpha");
    f.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn copies_the_selected_file_to_the_prefilled_destination() -> anyhow::Result<()> {
    let mut f = Fixture::new("alpha.txt", "alpha")?;
    f.open_explorer().await?;
    f.keys("alpha").await?;
    f.keys("<A-c><ret>").await?;
    assert_eq!(std::fs::read_to_string(f.path("alpha copy.txt"))?, "alpha");
    f.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn deletes_only_after_confirmation() -> anyhow::Result<()> {
    let mut f = Fixture::new("alpha.txt", "alpha")?;
    f.open_explorer().await?;
    f.keys("alpha").await?;

    f.keys("<A-x><ret>").await?;
    assert!(f.path("alpha.txt").exists());
    assert!(f.status().contains("canceled"));

    f.keys("<A-x>y<ret>").await?;
    assert!(!f.path("alpha.txt").exists());
    f.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn yanks_the_selected_path() -> anyhow::Result<()> {
    let mut f = Fixture::new("alpha.txt", "alpha")?;
    f.open_explorer().await?;
    f.keys("alpha").await?;
    f.keys("<A-y>").await?;

    let register = f.app.editor.config().clipboard.default_yank_register;
    let expected = stdx::path::get_relative_path(&f.file)
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        f.app.editor.first_register_value(register).as_deref(),
        Some(expected.as_str())
    );
    f.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_to_modify_the_parent_directory_entry() -> anyhow::Result<()> {
    let mut f = Fixture::new("sub/file.txt", "file")?;
    f.open_explorer().await?;
    f.keys("..").await?;
    f.keys("<A-x>").await?;

    assert!(f.path("sub").is_dir());
    assert!(f.status().contains("parent directory entry"));
    f.finish().await;
    Ok(())
}
