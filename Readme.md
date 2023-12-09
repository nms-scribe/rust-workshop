**Rust-workshop** is a task processing crate, for when Make, Just, or whatever your system, is too frustrating to figure out and you just want to get it done using rust.

Those things might be fine for simple build tasks. But when you need more complexity, who wants to browse through documentation looking for the right syntax when you could just write it in rust. Declarative code written in a configuration syntax is great and all. But when you just need it done right now, imperative is much easier. Eventually, any task management requires a real programming language, and turning configuration files into a programming language makes no sense, when programming languages already exist.

# Installation

This isn't on `crates.io` or anything like that, yet. If you want to use this, you have to install it from the github URL. Just look up on the top of this site, you'll find that somewhere. I can't be bothered to tell you how to get that into your cargo dependencies.

# Usage

Rust-workshop is best used with [rust-script](https://rust-script.org/). That way you don't need to have a separate rust project for your build code. But, I'm not stopping you from using it in a full rust project. Do whatever you want. 

Just put the dependency in your header, as one does with rust-script. Then use `rust-workshop::Workshop` and `rust-workshop::task`. Make a `main` function. Create a workshop struct with `Workshop::new`, add tasks to it with `Workshop::add` and the `task!` macro. Finally, run the `Workshop::main` function. There is some flexibility in how you use it if you don't want to do it that way. The code is simple, and all in one `lib.rs`. Just look there.

