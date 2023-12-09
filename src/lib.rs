/*!
rust-workshop is a task processing crate, for when makefile, justfile, or whatever your system is is just too frustrating to work with and you just want to get it done using rust rather than learn a new language.
*/


use core::fmt::Display;

use std::error::Error;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;
use std::fs::metadata;
use std::ffi::OsString;

use indoc::printdoc;
use gumdrop::Options;
use gumdrop::ParsingStyle;
// re-export 'duct' because it takes care of simplifying command lines and piping.
pub use duct; 
use duct::Expression;
// re-export 'glob' in case the user needs to use it for something more interesting than what I use it for.
pub use glob;
// This has to be renamed to avoid conflict with the crate it is found in.
use glob::glob as glob_glob;

#[derive(Debug)]
/// An error which could be returned during processing of a task's command. This is wrapped in [WorkshopError::Command].
pub enum CommandError {
    /// An i/o error caused by a failure to run a command process
    Process(std::io::Error),
    /// An error returned from a command function
    Function(Box<dyn Error>),
}

impl Error for CommandError {

}


impl Display for CommandError {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(err) => write!(f,"{err}"),
            Self::Function(err) => write!(f,"from function: {err}"),
        }
    }
}



#[derive(Debug)]
/// An error which could be returned during testing if a task should be skipped. This is wrapped in [WorkshopError::Skip].
pub enum SkipError {
    /// An i/o error caused by a failure to check information about files.
    IOError(std::io::Error),
    /// An error returned from a skip function
    Function(Box<dyn Error>),
}

impl Error for SkipError {

}


impl Display for SkipError {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipError::IOError(err) => write!(f,"{err}"),
            SkipError::Function(err) => write!(f,"from function: {err}"),
        }
    }
}


#[derive(Debug)]
/// An error associated with [Workshop].
pub enum WorkshopError {
    /// This is returned if an error occurs while checking if a task should be skipped.
    Skip(String,SkipError),
    /// This is returned if an error occurs while running a task's command.
    Command(String, CommandError),
    /// This is returned by [Workshop::glob] if an invalid pattern is checked.
    GlobPattern(glob::PatternError),
    /// This is returned by [Workshop::glob] if an error occurs while generating paths.
    Glob(glob::GlobError),
    /// This is returned by [Workshop::add] if the task already exists.
    TaskAlreadyExists(String),
    /// This is returned during dependency checking if a task is referenced that does not exist.
    TaskDoesNotExist(String),
    /// This is returned by [Workshop::run] and [Workshop::main] if the arguments were parsed incorrectly.
    Arguments(gumdrop::Error),
    /// This is returned during dependency checking if a task is found to have a cyclical dependency.
    CyclicalDependency(String),
    /// This is returned during dependency checking if a task is found, but is marked internal and can't be referenced from the command-line.
    TaskIsNotAvailable(String),
}

impl Error for WorkshopError {

}

impl Display for WorkshopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skip(name,err) => write!(f,"While checking if task '{name}' should be skipped: {err}"),
            Self::Command(name,err) => write!(f,"While running command in task '{name}': {err}"),
            Self::GlobPattern(err) => write!(f,"{err}"),
            Self::Glob(err) => write!(f,"{err}"),
            Self::TaskAlreadyExists(name) => write!(f,"Task '{name}' already exists."),
            Self::TaskDoesNotExist(name) => write!(f,"Task '{name}' does not exist."),
            Self::Arguments(err) => write!(f,"{err}"),
            Self::CyclicalDependency(name) => write!(f,"Task '{name}' depends on itself."),
            Self::TaskIsNotAvailable(name) => write!(f,"Task '{name}' is not available."),
        }
    }
}


impl From<glob::PatternError> for WorkshopError {

    fn from(value: glob::PatternError) -> Self {
        Self::GlobPattern(value)
    }
}

impl From<glob::GlobError> for WorkshopError {

    fn from(value: glob::GlobError) -> Self {
        Self::Glob(value)
    }
}

impl From<gumdrop::Error> for WorkshopError {

    fn from(value: gumdrop::Error) -> Self {
        Self::Arguments(value)
    }
}

/// A Skip object represents a method used for checking if a task should be skipped.
pub enum Skip {
    /// In this method, a function is called which returns a bool or an error. If `Ok(true)` is returned, the task will be skipped. If an error is returned, the whole process will fail.
    Function(Box<dyn Fn() -> Result<bool,Box<dyn Error>>>),
    /// In this method, the modified times of the files in `source` are compared to those of `target`. If all of the files in `source` are older than the newest file in `target`, the task shall be skipped.
    IfOlderThan{
        source: Vec<PathBuf>,
        target: Vec<PathBuf>
    }
}

impl Skip {

    /// Runs the appropriate skip process. Returns true if the task must be skipped, false if it shouldn't. Any error which occurs during the processing will cause an error to return.
    pub fn must_skip(&self, trace: bool) -> Result<bool, SkipError> {
        match self {
            Skip::Function(function) => (function)().map_err(SkipError::Function),
            Skip::IfOlderThan { source, target } => Self::must_skip_if_older_than(source,target,trace).map_err(SkipError::IOError)
        }
    }

    /// Checks the files in `source` against the files in `target`. If all of the files in `source` are older than the newest file in `target`, true is returned. Otherwise, false is returned, unless an IO error occurs while checking the data.
    pub fn must_skip_if_older_than(source: &[PathBuf], target: &[PathBuf], trace: bool) -> Result<bool, std::io::Error> {
        let source_time = Self::get_max_modified_time_for(source)?;
        if trace {
            println!("source time: {source_time:?}");
        }
        let target_time = Self::get_max_modified_time_for(target)?;
        if trace {
            println!("target time: {target_time:?}");
        }
        let skip = source_time < target_time;
        if trace {
            if skip {
                println!("source time is less than target time, will skip.")
            } else {
                println!("source time is not less than target time, will not skip.")
            }
        }

        Ok(skip)

    }

    /// Finds the timestamp of the newest file in the specified list, and returns that timestamp. Returns none if the list is empty or if the timestamp can not be found. If an io error occurs during checking, that is returned.
    pub fn get_max_modified_time_for(source: &[PathBuf]) -> Result<Option<SystemTime>, std::io::Error> {
        let mut result = None;
        for source in source {
            let source_time = metadata(source)?.modified()?;
            result = result.max(Some(source_time));
        }
        Ok(result)
    }

}

// The enum is much easier to work with than trying to manipulate traits.
/// Represents a method for running a command for a task.
pub enum Command {
    /// The command is encapsulated in a function that takes no parameters, and returns a result.
    Function(Box<dyn FnMut() -> Result<(),Box<dyn Error>>>),
    // You know, I could technically turn this into a function as well... I just feel like the
    // abstraction isn't necessary and the additional 'dyn' boundary might complicate things. Plus more useful error...
    /// The command is a subprocess command, represented by [duct::Expression]. Use [duct::cmd()] or [duct::cmd!()] to build this expression, and functions on `Expression` to add things like piping and redirecting.
    Command(Expression)
}

impl Command {

    /// Runs a [duct::Expression], returning a `Result` based on the success of running that.
    pub fn run_process(command: &Expression) -> Result<(),std::io::Error> {
        // NOTE: If the command was built with Expression::stdout_capture or Expression::stderr_capture, that data will be discarded.
        // The user shouldn't do this. If they truly want to not output results, they should use stdout_null or stderr_null. If they
        // planned on capturing the data to a variable, they may want to add a function to the task which does this, instead.
        // NOTE: By default, duct returns an error on non-zero status. However, the user could change that by marking it as unchecked.
        // I should allow them to do this. Duct will still error out on a killed process, so that's not a problem.

        command.run()?;

        Ok(())
    }

    /// Runs the appropriate command. If `Err` is returned, the entire process will fail.
    pub fn run(&mut self) -> Result<(),CommandError> {
        match self {
            Self::Function(function) => (function)().map_err(CommandError::Function),
            Self::Command(expression) => Self::run_process(expression).map_err(CommandError::Process)
        }
    }
}


#[allow(non_snake_case)] // gumdrop does not allow me to change the name of the positional argument "TASKS" field in help, so this is the only way to capitalize it. The allow tag will not work directly on the field, probably because of the derive trait.
#[derive(Options)]
/// Command line arguments in [Workshop::run] and [Workshop::main] are parsed into this object, with the help of [gumdrop::Options].
pub struct ProgramOptions {
    #[options(help = "Print help")]
    help: bool,

    #[options(help = "List available tasks")]
    list: bool,

    #[options(short = "V", help = "Print rust-workshop version")]
    version: bool,

    #[options(short = "t", help = "Output trace messages for commands.")]
    trace: bool,

    #[options(short = "T", help = "Output trace messages for dependency calculation.")]
    trace_dependencies: bool,


    #[options(free,help = "Tasks to run")]
    TASKS: Vec<String>

}

/// This represents a task that workshop is expected to run. Use [Task::new] or [task!] to create a task, and add it with [Workshop::add].
pub struct Task {
    help: String,
    dependencies: Vec<String>,
    command: Vec<Command>,
    skip: Option<Skip>,
    internal: bool
}

impl Task {

    /// Creates a new task with default values. A default task has no dependencies, no commands, can not be skipped, and is not internal.
    pub fn new<Help: Into<String>>(help: Help) -> Self {
        Self {
            help: help.into(),
            dependencies: Vec::new(),
            command: Vec::new(),
            skip: None,
            internal: false
        }

    }

    /// Adds the name of another task as a dependency to the current task.
    pub fn dependency<Dependency: Into<String>>(&mut self, dependency: Dependency) {
        self.dependencies.push(dependency.into());
    }

    /// Adds several dependencies into the current task.
    pub fn dependencies<Dependency: Into<String> + Clone>(&mut self, dependencies: &[Dependency]) {
        for dependency in dependencies {
            self.dependencies.push(dependency.clone().into());
        }
    }

    fn add_command(&mut self, command: Command) {
        self.command.push(command);
    }

    /// Adds a function command to the task. See [Command::Function]. Multiple commands can be added, they are executed in order of insertion.
    pub fn function<Function: FnMut() -> Result<(),Box<dyn Error>> + 'static>(&mut self, function: Function) {
        self.add_command(Command::Function(Box::from(function)))
    }

    /// Adds a process command to the task. See [Command::Command]. Multiple commands can be added, they are executed in order of insertion.
    pub fn command(&mut self, command: Expression) {
        self.add_command(Command::Command(command))
    }

    /// Tells the task that it should skip if the files in `source` are older than the files in `target`. See [Skip::IfOlderThan].
    pub fn skip_if_olderthan<Path: Into<PathBuf> + Clone>(&mut self, source: &[Path], target: &[Path]) {
        let source = source.iter().map(|path| path.clone().into()).collect();
        let target = target.iter().map(|path| path.clone().into()).collect();
        self.skip = Some(Skip::IfOlderThan { source, target });
    }

    /// Tells the task that it should skip based on the result of the specified function. See [Skip::Function].
    pub fn skip<Function: Fn() -> Result<bool,Box<dyn Error>> + 'static>(&mut self, function: Function) {
        self.skip = Some(Skip::Function(Box::from(function)));
    }

    /// Marks the task as internal. Internal tasks can not be called from the command line, nor are they listed. However, they may be used as dependencies of non-internal tasks.
    pub fn internal(&mut self) {
        self.internal = true;
    }

}

#[macro_export]
/// The task macro allows you to use something like a struct-constructor syntax to create a task. A 'help' property is the only one required. The other available properties are the same as the methods on [Task], and the values are passed to those functions. The `skip_if_older_than` method can be used by wrapping the two arguments in parentheses, like a tuple.
macro_rules! task {
    (@key $task: ident, skip_if_olderthan, $value: expr) => {
        $task.skip_if_olderthan($value.0,$value.1)
    };
    (@key $task: ident, $key: ident, $value: expr) => {
        $task.$key($value)
    };
    (help: $help: expr, $($key: ident: $value: expr),* $(,)?) => {{
        let mut task = $crate::Task::new($help);
        $(
            task!(@key task, $key, $value);
        )*
        task
    }};
}



#[derive(Default)]
/**
A Workshop object represents a set of tasks to run, and contains functionality for building and running these tasks. 

The easiest way to work with a workshop is by creating a rust-script, or a full-fledged binary project if you want to. In the main function of your script, create the workshop with [Workshop::new], add tasks to it with [Workshop::add] and the [task!] macro, and then call [Workshop::main], which will collect the arguments from the command line.
*/
pub struct Workshop {
    tasks: HashMap<String,Task>
}

impl Workshop {

    /// Returns a default Workshop, with no tasks.
    pub fn new() -> Self {
        Self::default()
    }

    // NOTE: The globbing works when it's called, so the files would have to exist when the 
    // tasks are built, unless you use a function task and call it in there. I'm not sure of a way to make this easier without
    // recreating the 'cmd' function and macros to take enums that allow for glob patterns, and expand
    // them on run. However, I also am not sure I see the need for glob patterns in this type of tool:
    // it's better to have a known list of files to run a task on, rather than hope that the user doesn't
    // add an extra file which would then have to be processed.
    /// This associated function can be used to turn a number of glob patterns (ex: `*.txt`) into a list of something that can be turned into a path. For more information, and access to more related functionality, see [glob].
    pub fn glob<Something: From<PathBuf>,Pattern: AsRef<str>>(patterns: &[Pattern]) -> Result<Vec<Something>,WorkshopError> {
        let mut result = Vec::new();
        for pattern in patterns {
            for path in glob_glob(pattern.as_ref())? {
                result.push((path?).into())
            }
        }
        Ok(result)

    }

    /// This associated function can be used to turn a number of glob patterns (ex: `*.txt`) into a list of paths. For more information, and access to more related functionality, see [glob].
    pub fn glob_to_paths<Pattern: AsRef<str>>(patterns: &[Pattern]) -> Result<Vec<PathBuf>,WorkshopError> {
        Self::glob(patterns)
    }

    /// This associated function can be used to turn a number of glob patterns (ex: `*.txt`) into a list of strings. For more information, and access to more related functionality, see [glob].
    pub fn glob_to_osstrings<Pattern: AsRef<str>>(patterns: &[Pattern]) -> Result<Vec<OsString>,WorkshopError> {
        Self::glob(patterns)
    }

    /// Call this function to add a new task by name.
    pub fn add<Name: Into<String> + Display>(&mut self, name: Name, task: Task) -> Result<(),WorkshopError> {
        let name = name.into();
        if self.tasks.insert(name.clone(), task).is_some() {
            Err(WorkshopError::TaskAlreadyExists(name))
        } else {
            Ok(())
        }

    }

    /// Call this function to run tasks directly. Specify the tasks to run under `tasks`. If `trace_dependencies` is true, messages will be printed to stdout during dependency calculation. If `trace_commands` is true, messages will be printed to stdout during command processing.
    pub fn run_tasks<Task: AsRef<str>>(&mut self, tasks: &[Task], trace_dependencies: bool, trace_commands: bool) -> Result<(),WorkshopError> {

        let task_list = self.calculate_dependency_list(tasks, trace_dependencies)?;

        for name in task_list {
            if let Some(task) = self.tasks.get_mut(&name) {

                let skip = if let Some(skip) = &task.skip {
                    if trace_commands {
                        println!("Checking if task '{name}' should be skipped.");
                    }
                    skip.must_skip(trace_commands).map_err(|err| WorkshopError::Skip(name.clone(),err))?  
                } else {
                    false
                };

                if !skip {
                    if trace_commands {
                        println!("Running task '{name}'.");
                    }

                    for command in &mut task.command {
                        command.run().map_err(|err| WorkshopError::Command(name.clone(), err))?
                    }
                } else if trace_commands {
                    println!("Skipping task '{name}'.");
                }
            } else {
                return Err(WorkshopError::TaskDoesNotExist(name));
            }

        }

        Ok(())
    }


    /// Call this function to run the workshop using an list of string arguments. This can be useful if you need to programatically supply your own arguments.
    pub fn run<Arg: AsRef<str>>(&mut self, args: &[Arg]) -> Result<(),WorkshopError> {

        let mut options = ProgramOptions::parse_args(args, ParsingStyle::AllOptions)?;

        if options.help {
            self.show_help(&options.TASKS);
        } 
        
        if options.list {
            self.show_list();
        } 
        
        if options.version {
            self.show_version();
        }

        if options.TASKS.is_empty() {
            options.TASKS.push("default".to_owned())
        }
        
        if !options.help && !options.version && !options.list {
            self.run_tasks(&options.TASKS,options.trace_dependencies,options.trace)
        } else {
            Ok(())
        }

    }

    /// Call this function to run the workshop with the arguments passed to the command line.
    pub fn main(&mut self) -> Result<(),Box<dyn Error>> {
        // NOTE: I use a Result<(),Box<dyn Error>> so that the same can be done in the main function of a script,
        // and that script still be able to use the '?' on things like adding a task. I'm also converting it into 
        // a string here so that the error messages are easier to read (default output if I don't do this is debug).
        self.run(&std::env::args().skip(1).collect::<Vec<_>>()).map_err(|err| format!("{err}").into())
    }

    /// Retrieves the executable name for use in printing help. This functions supports retrieving the script name if the library is run via rust-script.
    pub fn get_executable_name() -> String {
        // support rust-script:
        let path = if let Ok(path) = std::env::var("RUST_SCRIPT_PATH") {
            std::path::PathBuf::from(path)
        } else {
            std::env::current_exe().expect("I should have been able to get the executable name.")
        };
        if let Some(name) = path.file_name() {
            name.to_string_lossy().into()
        } else {
            "workshop".into()
        }

    }

    /// Print the help information to stdout.
    pub fn show_help<Task: AsRef<str> + Display>(&mut self, tasks: &[Task]) {

        let executable = Self::get_executable_name();
        if tasks.is_empty() {
            let usage = ProgramOptions::usage();
            printdoc!("
                A task automator

                Usage: {executable} [OPTIONS] [TASKS]

                {usage}

                See '{executable} help <task>' for more information on a specific task

            ");
        } else {
            for key in tasks {
                if let Some(value) = self.tasks.get(key.as_ref()) {
                    if !value.internal {
                        let help = &value.help;
                        println!("{key}\t{help}")
                    } else {
                        println!("'{key}' is not an available task.")
                    }
                } else {
                    println!("'{key}' is not an available task.")
                }
            }
        }
    }

    /// List the available tasks at the command line.
    pub fn show_list(&self) {
        println!("Available Tasks");
        if self.tasks.is_empty() {
            println!("  -- none --")
        } else {
            for (key,value) in &self.tasks {
                if !value.internal {
                    let help = &value.help;
                    println!("{key}:\n {help}")
                }
            }
    
        }
        println!()
    }

    /// Show the version of rust-workshop.
    pub fn show_version(&self) {
        println!("{} version: {}",env!("CARGO_PKG_NAME"),env!("CARGO_PKG_VERSION"));
    }

    // No reason not to make this public. If the user wants to do some testing.
    /// This is called automatically by [Workshop::main] and [Workshop::run] to calculate dependencies for tasks from the command line. It returns a list of tasks which must be run to accomplish the passed tasks. The `trace` parameter specifies whether trace messages will be logged to stdout during this checking.
    pub fn calculate_dependency_list<Task: AsRef<str>>(&self, tasks: &[Task], trace: bool) -> Result<Vec<String>,WorkshopError> {
        let mut checker = DependencyChecker::new(self, trace);

        for task in tasks {
            checker.require_task(task.as_ref())?;
        }

        Ok(checker.into_tasks())


    }
}

/// This is used by the workshop to calculate dependencies. It is intended for advanced usage only.
pub struct DependencyChecker<'workshop> {
    workshop: &'workshop Workshop,
    marked: HashSet<String>,
    cyclical_check: HashSet<String>,
    tasks: Vec<String>,
    trace: bool
}

impl DependencyChecker<'_> {

    /// Creates a new DependencyChecker based on the specified workshop, enabling tracing if `trace` is true.
    pub fn new(workshop: &Workshop, trace: bool) -> DependencyChecker {
        DependencyChecker {
            workshop,
            marked: HashSet::new(),
            cyclical_check: HashSet::new(),
            tasks: Vec::new(),
            trace
        }
    }

    fn trace(&self, indent: &str, message: String) {
        if self.trace {
            println!("{indent}{message}")
        }
    }

    fn visit_dependencies(&mut self, node: &str, first_level: bool, indent: &str) -> Result<(),WorkshopError> {
        self.trace(indent,format!("Visiting dependent task {node}"));
        if self.marked.contains(node) {
            self.trace(indent,format!("Task {node} already checked."));
            Ok(()) 
        } else if self.cyclical_check.contains(node) {
            Err(WorkshopError::CyclicalDependency(node.to_owned()))
        } else if let Some(task_info) = self.workshop.tasks.get(node) {

            if first_level && task_info.internal {

                // this was a task "picked" by the user, but it is marked as internal and therefore not supposed to be called directly.
                Err(WorkshopError::TaskIsNotAvailable(node.to_owned()))

            } else {

                self.trace(indent,format!("Marking task {node} for cyclical check."));
                self.cyclical_check.insert(node.to_owned());

                for task in &task_info.dependencies {
                    self.visit_dependencies(task,false,&format!("  {indent}"))?;
                }
    
                self.trace(indent,format!("Unmarking task {node} for cyclical check."));
                // to avoid allocation, take the node that we just cloned before.
                let node = self.cyclical_check.take(node).expect("This was just inserted, it should still be here.");
    
                self.trace(indent,format!("Marking task {node} as checked."));
                // If I had some sort of map that maintained an order of insert, I wouldn't need a separate marked set and list of tasks. But I feel like adding that sort of crate in would be overkill.
                self.marked.insert(node.clone());
    
                self.trace(indent,format!("Adding task {node} to list."));
                // all of its dependencies are already on the list, so this can go on now.
                self.tasks.push(node);
    
                Ok(()) 

            }


        } else {
            Err(WorkshopError::TaskDoesNotExist(node.to_owned()))
        }



    }    

    /// Checks the dependencies of the specified task, and adds it and them to the list in an appropriate order, if they aren't already on the list. The list can be retrieved with take_output.
    pub fn require_task(&mut self, node: &str) -> Result<(),WorkshopError> {
        let indent = String::new();
        self.trace(&indent,format!("Requiring task {node}."));
        self.visit_dependencies(node, true, &indent)
    }

    /// Drops the checker and returns the generated list of tasks, as built during calls to [require_task].
    fn into_tasks(self) -> Vec<String> {
        self.tasks
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn calculate_dependencies() {

        let result = Rc::new(RefCell::new(Vec::new()));

        let mut workshop = Workshop::new();

        // FUTURE: How common is this pattern? Should I add some sugar to support it?
        let output = result.clone();
        workshop.add("5", task!{
            help: "Five",
            dependency: "11",
            function: move || {output.borrow_mut().push("5"); Ok(( ))}
        }).expect("Task should have been added.");
        
        let output = result.clone();
        workshop.add("7", task!{
            help: "Seven",
            dependencies: &["11","8"],
            function: move || {output.borrow_mut().push("7"); Ok(( ))},
        }).expect("Task should have been added.");

        let output = result.clone();
        workshop.add("3", task!{
            help: "Three",
            dependencies: &["10","8"],
            function: move || {output.borrow_mut().push("3"); Ok(( ))},
        }).expect("Task should have been added.");

        let output = result.clone();
        workshop.add("11",task!{
            help: "Eleven",
            dependencies: &["2","9","10"],
            function: move || {output.borrow_mut().push("11"); Ok(( ))},
        }).expect("Task should have been added.");


        let output = result.clone();
        workshop.add("8", task!{
            help: "Eight",
            dependencies: &["9"],
            function: move || {output.borrow_mut().push("8"); Ok(( ))},
        }).expect("Task should have been added.");

        let output = result.clone();
        workshop.add("2", task!{
            help: "Two",
            function: move || {output.borrow_mut().push("2"); Ok(( ))}
        }).expect("Task should have been added.");

        let output = result.clone();
        workshop.add("9", task!{
            help: "Nine",
            function: move || {output.borrow_mut().push("9"); Ok(( ))}
        }).expect("Task should have been added.");

        let output = result.clone();
        workshop.add("10", task!{
            help: "Ten",
            function: move || {output.borrow_mut().push("10"); Ok(( ))},
        }).expect("Task should have been added.");

        workshop.run_tasks(&["5","8"],false,false).expect("Tasks should have run.");

        assert_eq!(result.take(),vec!["2","9","10","11","5","8"]);

    }
}
