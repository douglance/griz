//! Typed search results retain their shape until the framework serializes them.
use crate::context::CmdError;
use crate::verdict::{Outcome, unmet};
use griz_core::{FindFilesPage, FindPage, FindQuery, find, find_files};
use schemars::JsonSchema;
use serde::Serialize;
#[derive(Serialize, JsonSchema)]
pub(crate) struct FindResponse {
    #[serde(flatten)]
    page: SearchPage,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
enum SearchPage {
    Matches(FindPage),
    Files(FindFilesPage),
}
pub(crate) fn search(
    query: &FindQuery,
    files_only: bool,
) -> Result<(FindResponse, usize), CmdError> {
    let (page, total) = if files_only {
        let page = find_files(query).map_err(CmdError::invalid)?;
        let total = page.total;
        (SearchPage::Files(page), total)
    } else {
        let page = find(query).map_err(CmdError::invalid)?;
        let total = page.total;
        (SearchPage::Matches(page), total)
    };
    match &page {
        SearchPage::Matches(page) => page
            .matches
            .iter()
            .try_for_each(|hit| serializable_path(&hit.path))?,
        SearchPage::Files(page) => page
            .file_matches
            .iter()
            .try_for_each(|hit| serializable_path(&hit.path))?,
    }
    Ok((
        FindResponse {
            page,
            outcome: None,
            reason: None,
        },
        total,
    ))
}
pub(crate) fn with_expectation(
    mut response: FindResponse,
    expected: usize,
    total: usize,
) -> (FindResponse, bool) {
    response.reason = unmet("matches", Some(expected), total);
    let failed = response.reason.is_some();
    response.outcome = Some(if failed {
        Outcome::Failed
    } else {
        Outcome::Passed
    });
    (response, failed)
}

fn serializable_path(path: &std::path::Path) -> Result<(), CmdError> {
    if path.to_str().is_some() {
        return Ok(());
    }
    serde_json::to_value(path)
        .map(|_| ())
        .map_err(|error| CmdError::invalid(error.to_string()))
}
