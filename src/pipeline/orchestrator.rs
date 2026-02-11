// Phase 10: 全ジョブ実行

use tracing::info;

use crate::pipeline::job_runner::{JobConfig, JobResult, run_job};

/// Run multiple jobs, collecting results.
/// One job failure does NOT prevent other jobs from running.
pub fn run_all_jobs(jobs: &[JobConfig]) -> Vec<crate::error::Result<JobResult>> {
    info!(job_count = jobs.len(), "starting job execution");
    let results: Vec<_> = jobs.iter().map(run_job).collect();
    let succeeded = results.iter().filter(|r| r.is_ok()).count();
    let failed = results.iter().filter(|r| r.is_err()).count();
    info!(succeeded, failed, "all jobs finished");
    results
}
