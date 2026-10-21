use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn move_selection_single_selection_up() -> anyhow::Result<()> {
    test((
        indoc! {"
            aaaaaa
            bbbbbb
            cc#[|c]#ccc
            dddddd
            "},
        "<C-k>",
        indoc! {"
            aaaaaa
            cc#[|c]#ccc
            bbbbbb
            dddddd
            "},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_selection_single_selection_down() -> anyhow::Result<()> {
    test((
        indoc! {"
            aa#[|a]#aaa
            bbbbbb
            cccccc
            dddddd
            "},
        "<C-j>",
        indoc! {"
            bbbbbb
            aa#[|a]#aaa
            cccccc
            dddddd
            "},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_selection_single_selection_top_up() -> anyhow::Result<()> {
    // If already on top of the file and going up, nothing should change.
    test((
        indoc! {"
            aa#[|a]#aaa
            bbbbbb
            cccccc
            dddddd"},
        "<C-k>",
        indoc! {"
            aa#[|a]#aaa
            bbbbbb
            cccccc
            dddddd"},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_selection_block_up() -> anyhow::Result<()> {
    test((
        indoc! {"
            aaaaaa
            bb#[bbbb
            ccc|]#ccc
            dddddd
            eeeeee
            "},
        "<C-k>",
        indoc! {"
            bb#[bbbb
            ccc|]#ccc
            aaaaaa
            dddddd
            eeeeee
            "},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_selection_block_down() -> anyhow::Result<()> {
    test((
        indoc! {"
            #[|aaaaaa
            bbbbbb
            ccc]#ccc
            dddddd
            eeeeee
            "},
        "<C-j>",
        indoc! {"
            dddddd
            #[|aaaaaa
            bbbbbb
            ccc]#ccc
            eeeeee
            "},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_two_cursors_down() -> anyhow::Result<()> {
    test((
        indoc! {"
            aaaaaa
            bb#[|b]#bbb
            cccccc
            d#(dd|)#ddd
            eeeeee
            "},
        "<C-j>",
        indoc! {"
            aaaaaa
            cccccc
            bb#[|b]#bbb
            eeeeee
            d#(dd|)#ddd
            "},
    ))
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn move_selection_preserves_multibyte_cursor_offset() -> anyhow::Result<()> {
    test((
        indoc! {"
            aaaaaa
            ααα#[|α]#αα
            cccccc
            "},
        "<C-k>",
        indoc! {"
            ααα#[|α]#αα
            aaaaaa
            cccccc
            "},
    ))
    .await?;
    Ok(())
}
