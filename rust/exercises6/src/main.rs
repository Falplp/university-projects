
fn main() {
    // TODO 1 (warm-up): Figure out how many cores does your PC have (with Rust code :)) and print it
    // You don't need an external crate for it, there is a function for it in the stdlib

    // TODO 2: Measure the CPU utilization of each core on your computer, e.g. using the
    // [psutil](https://docs.rs/psutil) crate. Examine the API of the crate carefully!
    // To figure out how to actually find out what is the current CPU utilization.
    // Then print the utilization every second.
    // You can use the `stress -c <number-of-cores>` command on Linux to generate some "heat".

    // TODO 3: Implement a simple "progress bar" that will render the usage for each core in a
    // visual way. You can use ANSI sequences or "\r" to print it repeatedly on the same line.
    // E.g. [xxxxx.....]

    // TODO 4: Make the progress bar colored, based on the utilization.
    // Utilization < 30% should be green, < 70% should be yellow, and higher utilization should be
    // red. Try to find a crate that will help you with coloring strings.

    // TODO 5: create a TUI application that will draw a historical chart of average CPU usage.
    // Display the current per-core CPU usage next to the chart.
    // Use the [ratatui](https://ratatui.rs/tutorials/hello-ratatui/) crate.
    // Warning: if you want to draw the per-cope "htop progress bars" with ratatui, don't combine
    // other coloring crates with Ratatui; use Ratatui's colors and styles instead.
}
