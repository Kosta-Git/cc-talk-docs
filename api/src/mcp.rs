use rmcp::{handler::server::wrapper::Parameters, serde_json, tool, tool_router};

use crate::{SharedState, document::DocumentParams, query::QueryParams};

#[derive(Clone)]
pub struct DocsMcp {
    pub state: SharedState,
}

#[tool_router(server_handler)]
impl DocsMcp {
    #[tool(description = "Search the ccTalk documentation")]
    fn search_docs(&self, Parameters(params): Parameters<QueryParams>) -> Result<String, String> {
        let hits = crate::query::search_docs(&params, &self.state).map_err(|e| e.to_string())?;
        serde_json::to_string(&hits).map_err(|error| error.to_string())
    }

    #[tool(description = "Retrieve raw page content from the ccTalk documentation")]
    fn get_doc(&self, Parameters(params): Parameters<DocumentParams>) -> Result<String, String> {
        let pages =
            crate::document::get_document(&params, &self.state).map_err(|e| e.to_string())?;
        serde_json::to_string(&pages).map_err(|error| error.to_string())
    }
}
