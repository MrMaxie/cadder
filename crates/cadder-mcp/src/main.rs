use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = cadder_mcp::Args::parse();
  cadder_mcp::CadderMcpServer::new(args)?.run_stdio().await
}
