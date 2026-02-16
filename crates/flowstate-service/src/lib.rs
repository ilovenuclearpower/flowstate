mod http;
mod local;
mod traits;

pub use http::HttpService;
pub use local::LocalService;
pub use traits::{ServiceError, TaskService};
