use ayrola_kernel::llm::{Llm, LlmBackend};

fn main() {
    let llm = Llm::new(LlmBackend::NineRouter);
    match llm.query("Is the sky blue?") {
        Ok(resp) => {
            println!("Backend: {:?}", resp.backend);
            println!("Content: {}", resp.content);
            println!("Duration: {}ms", resp.duration_ms);
            println!("Tokens: {} in / {} out", resp.input_tokens, resp.output_tokens);
            println!("Cost: ${}", resp.cost_usd);
        }
        Err(e) => println!("ERROR: {}", e),
    }
}
