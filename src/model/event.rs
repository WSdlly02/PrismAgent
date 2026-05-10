use crate::model::asyncioinstance::{IoError, IoOutput};
pub enum ShellEvent {
    CommandInput {
        command: CommandEvent,
    },
    UserInput {
        run_id: String,
        agent_id: String,
        content: String,
    },
    LLMInput {
        run_id: String,
        agent_id: String,
        content: String,
    },
    Cancel {
        run_id: String,
        agent_id: String,
    },
    Shutdown,
}
pub enum CommandEvent {
    NewRun { title: String },
    ResumeRun { run_id: String },
    DeleteRun { run_id: String },
    ListRuns,
}
pub enum KernelEvent {
    Output {
        run_id: String,
        agent_id: String,
        content: IoOutput,
    },
    Error {
        run_id: String,
        agent_id: String,
        error: IoError,
    },
    Done {
        run_id: String,
        agent_id: String,
    },
}
