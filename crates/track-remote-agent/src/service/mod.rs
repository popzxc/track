mod dispatch;
mod remote_agent_services;
mod review;

pub use self::dispatch::RemoteDispatchService;
pub use self::remote_agent_services::{RemoteAgentConfigProvider, RemoteAgentServices};
pub use self::review::RemoteReviewService;

#[cfg(test)]
pub(crate) use self::remote_agent_services::StaticRemoteAgentConfigService;

#[cfg(test)]
mod tests;
