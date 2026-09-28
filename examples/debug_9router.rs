use ayrola_kernel::llm::{Llm, LlmBackend};

fn main() {
    let llm = Llm::new(LlmBackend::NineRouter);
    match llm.query("Is the sky blue? Answer yes or no.") {
        Ok(resp) => {
            println!("Backend: {:?}", resp.backend);
            println!("Content: {:?}", resp.content);
            println!("Empty: {}", resp.content.is_empty());
            println!("Lower contains 'yes': {}", resp.content.to_lowercase().contains("yes"));
            println!("Lower contains 'no': {}", resp.content.to_lowercase().contains("no"));
            println!("Duration: {}ms", resp.duration_ms);
            println!("Tokens: {} in / {} out", resp.input_tokens, resp.output_tokens);
        }
        Err(e) => {
            println!("ERROR: {}", e);
        }
    }
}
