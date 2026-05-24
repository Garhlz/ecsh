use std::sync::atomic::{AtomicBool, Ordering};

static OUTPUT_OCCURRED: AtomicBool = AtomicBool::new(false);
static OUTPUT_ENDED_WITH_NEWLINE: AtomicBool = AtomicBool::new(true);

pub fn reset_output_state() {
    OUTPUT_OCCURRED.store(false, Ordering::Relaxed);
    OUTPUT_ENDED_WITH_NEWLINE.store(true, Ordering::Relaxed);
}

pub fn note_output(text: &str, newline: bool) {
    OUTPUT_OCCURRED.store(true, Ordering::Relaxed);
    OUTPUT_ENDED_WITH_NEWLINE.store(newline || text.ends_with('\n'), Ordering::Relaxed);
}

pub fn output_needs_newline() -> bool {
    OUTPUT_OCCURRED.load(Ordering::Relaxed) && !OUTPUT_ENDED_WITH_NEWLINE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::{note_output, output_needs_newline, reset_output_state};

    #[test]
    fn no_output_does_not_need_newline() {
        reset_output_state();
        assert!(!output_needs_newline());
    }

    #[test]
    fn print_without_newline_needs_separator() {
        reset_output_state();
        note_output("123", false);
        assert!(output_needs_newline());
    }

    #[test]
    fn print_with_newline_does_not_need_separator() {
        reset_output_state();
        note_output("123", true);
        assert!(!output_needs_newline());
    }
}
