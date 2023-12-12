/*!
rust-workshop is a task processing crate, for when makefile, justfile, or whatever your system is is just too frustrating to work with and you just want to get it done using rust rather than learn a new language.
*/


use core::fmt::Display;

use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use std::fs::metadata;
use std::path::PathBuf;
use std::time::SystemTime;
use std::rc::Rc;
use std::cell::RefCell;
use std::cell::BorrowMutError;

use indoc::printdoc;
use gumdrop::Options;
use gumdrop::ParsingStyle;
// re-export 'duct' because it takes care of simplifying command lines and piping.
pub use duct; 
use duct::cmd;
use duct::Expression;
use duct::IntoExecutablePath;
// re-export 'glob' in case the user needs to use it for something more interesting than what I use it for.
pub use glob;

// TODO: I also need a "CompoundDependency" which lets me depend on one argument task multiple times with different arguments.
// TODO: I can use the following for templates...
macro_rules! test_template {
    ($($variable: ident),* { $($statement: expr);*  }) => {
        fn test_this(map: std::collections::HashMap<String,String>) -> usize {
            $(
                let $variable = map.get(stringify!($variable)).expect(concat!("Variable ",stringify!($variable)," was not available."));
            )*
            $($statement);*

        }
        
    };
}

test_template!(bar,foo,gee {
    println!("{bar}");
    println!("{foo}");
    println!("{gee}");
    if gee == "no" {
        println!("no")
    } else {
        println!("2")
    };
    23
});

#[derive(Debug)]
/// An error which could be returned during processing of a task's command. This is wrapped in [WorkshopError::Command].
pub enum CommandError {
    /// An i/o error caused by a failure to run a command process
    Process(std::io::Error),
    /// An error returned from a command function
    Function(Box<dyn Error>),
    /// An error caused by an inability to borrow a function
    FunctionBorrow(BorrowMutError)
}

impl Error for CommandError {

}


impl Display for CommandError {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(err) => write!(f,"{err}"),
            Self::Function(err) => write!(f,"from function: {err}"),
            Self::FunctionBorrow(_) => write!(f,"A function pointer was already borrowed.") // This might happen if someone attempts a recursive task.
        }
    }
}

impl From<BorrowMutError> for CommandError {
    fn from(value: BorrowMutError) -> Self {
        Self::FunctionBorrow(value)
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
/// An error returned while globbing a string
pub enum GlobError {
    PatternError(glob::PatternError),
    ParseError(glob::GlobError)
}

impl Error for GlobError {

}


impl Display for GlobError {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GlobError::PatternError(err) => write!(f,"{err}"),
            GlobError::ParseError(err) => write!(f,"{err}"),
        }
    }
}

impl From<glob::PatternError> for GlobError {

    fn from(value: glob::PatternError) -> Self {
        Self::PatternError(value)
    }
}

impl From<glob::GlobError> for GlobError {

    fn from(value: glob::GlobError) -> Self {
        Self::ParseError(value)
    }
}



#[derive(Debug)]
/// An error associated with [Workshop].
pub enum WorkshopError {
    /// This is returned if an error occurs while checking if a task should be skipped.
    Skip(String,SkipError),
    /// This is returned if an error occurs while running a task's command.
    Command(String, CommandError),
    /// This is returned by globbing functionality if an error occurs
    Glob(GlobError),
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
    /// This is returned if an attempt was made to use a task that was already mutably borrowed. It might happen if you attempt to run a task recursively.
    TaskBorrow(String),
    /// This is returned if an attempt was made to call a task that was already borrowed. It might happen if you attempt to run a task recursively.
    TaskBorrowMut(String),
}

impl Error for WorkshopError {

}

impl Display for WorkshopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skip(name,err) => write!(f,"While checking if task '{name}' should be skipped: {err}"),
            Self::Command(name,err) => write!(f,"While running command in task '{name}': {err}"),
            Self::Glob(err) => write!(f,"{err}"),
            Self::TaskAlreadyExists(name) => write!(f,"Task '{name}' already exists."),
            Self::TaskDoesNotExist(name) => write!(f,"Task '{name}' does not exist."),
            Self::Arguments(err) => write!(f,"{err}"),
            Self::CyclicalDependency(name) => write!(f,"Task '{name}' depends on itself."),
            Self::TaskIsNotAvailable(name) => write!(f,"Task '{name}' is not available."),
            Self::TaskBorrow(name) => write!(f,"Task '{name}' was referenced while being called."),
            Self::TaskBorrowMut(name) => write!(f,"Task '{name}' was called while still referenced elsewhere.")
        }
    }
}


impl From<glob::PatternError> for WorkshopError {

    fn from(value: glob::PatternError) -> Self {
        Self::Glob(value.into())
    }
}

impl From<glob::GlobError> for WorkshopError {

    fn from(value: glob::GlobError) -> Self {
        Self::Glob(value.into())
    }
}

impl From<gumdrop::Error> for WorkshopError {

    fn from(value: gumdrop::Error) -> Self {
        Self::Arguments(value)
    }
}

/// A Skip object represents a method used for checking if a task should be skipped.
#[derive(Clone)]
pub enum Skip {
    /// In this method, a function is called which returns a bool or an error. If `Ok(true)` is returned, the task will be skipped. If an error is returned, the whole process will fail.
    Function(Rc<dyn Fn() -> Result<bool,Box<dyn Error>>>),
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
        let source_time = Self::get_max_modified_time_for(source,false)?;
        if trace {
            println!("source time: {source_time:?}");
        }
        let target_time = Self::get_max_modified_time_for(target,true)?;
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

    /// Finds the timestamp of the newest file in the specified list, and returns that timestamp. Returns none if the list is empty or if the timestamp can not be found. If an io error occurs during checking, that is returned. Pass `false` to `ignore_missing` if you want it to ignore missing files, which probably should be done for target files.
    pub fn get_max_modified_time_for(source: &[PathBuf], ignore_missing: bool) -> Result<Option<SystemTime>, std::io::Error> {
        let mut result = None;
        for source in source {
            let metadata = metadata(source);
            let metadata = if ignore_missing && metadata.is_err() {
                continue;
            } else {
                metadata?
            };
            let source_time = metadata.modified()?;
            result = result.max(Some(source_time));
        }
        Ok(result)
    }

}

/// An enum which can either be a regular string, or a string meant to be a glob pattern. Use [glob_str] to create a pattern, or [Into] to convert a regular `string` or `&str` into a non-pattern string.
pub enum GlobString {
    Glob(String),
    String(String)
}

impl Into<GlobString> for String {
    fn into(self) -> GlobString {
        GlobString::String(self)
    }
}

impl Into<GlobString> for &str {
    fn into(self) -> GlobString {
        GlobString::String(self.to_owned())
    }
}

/// Use this command to create a pattern to be passed to [glob_cmd] or [glob_cmd!]
pub fn glob_str<Pattern: Into<String>>(pattern: Pattern) -> GlobString {
    GlobString::Glob(pattern.into())
}

/// Wraps [duct::cmd] to accept potential glob patterns ([GlobString]) as arguments. If a glob pattern is passed, it is expanded into paths and pushed onto the arguments for the resulting command. The pattern is evaluated at construction time, so if you have a task that needs to check at the time it runs, you would have to use a closure instead.
pub fn glob_cmd<Executable, Arguments>(program: Executable, glob_args: Arguments) -> Result<Expression,GlobError>
where
    Executable: IntoExecutablePath,
    Arguments: IntoIterator,
    Arguments::Item: Into<GlobString>,
{
    // I thought about turning this into an Iterator, except that the iterator has to return a result, no the OSString,
    // and since the cmd function doesn't handle errors in the iterator, that won't work.
    let mut args: Vec<OsString> = Vec::new();
    for arg in glob_args {
        match arg.into() {
            GlobString::Glob(glob) => for path in glob::glob(&glob)? {
                args.push(path?.into())
            },
            GlobString::String(string) => args.push(string.into()),
        }
    }
    Ok(cmd(program,args))
}

/// A replacement for [duct::cmd!] which accepts glob patterns created with [glob_str].
#[macro_export]
macro_rules! glob_cmd {
    ( $program:expr $(, $arg:expr )* $(,)? ) => {
        {
            let args: std::vec::Vec<$crate::GlobString> = std::vec![$( Into::<$crate::GlobString>::into($arg) ),*];
            $crate::glob_cmd($program, args)
        }
    };
}

// The enum is much easier to work with than trying to manipulate traits.
/// Represents a method for running a command for a task.
#[derive(Clone)]
pub enum Command {
    /// The command is encapsulated in a function that takes no parameters, and returns a result.
    Function(Rc<RefCell<dyn FnMut() -> Result<(),Box<dyn Error>>>>),
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
            Self::Function(function) => (function.try_borrow_mut()?)().map_err(CommandError::Function),
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
#[derive(Clone)]
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
    pub fn function<Function: FnMut() -> Result<(),Box<dyn Error>>+ 'static>(&mut self, function: Function) {
        self.add_command(Command::Function(Rc::from(RefCell::from(function))))
    }

    /// Adds a process command to the task. See [Command::Command]. Multiple commands can be added, they are executed in order of insertion.
    pub fn command(&mut self, command: Expression) {
        self.add_command(Command::Command(command))
    }

    /// Tells the task that it should skip if the files in `source` are older than the files in `target`. See [Skip::IfOlderThan].
    pub fn skip_if_older_than<Path: Into<PathBuf> + Clone>(&mut self, source: &[Path], target: &[Path]) {
        let source = source.iter().map(|path| path.clone().into()).collect();
        let target = target.iter().map(|path| path.clone().into()).collect();
        self.skip = Some(Skip::IfOlderThan { source, target });
    }

    /// Tells the task that it should skip based on the result of the specified function. See [Skip::Function].
    pub fn skip<Function: Fn() -> Result<bool,Box<dyn Error>>+ 'static>(&mut self, function: Function) {
        self.skip = Some(Skip::Function(Rc::from(function)));
    }

    /// Marks the task as internal. Internal tasks can not be called from the command line, nor are they listed. However, they may be used as dependencies of non-internal tasks.
    pub fn internal(&mut self) {
        self.internal = true;
    }

}

pub enum TaskEntry {
    Task(Task)
}
impl TaskEntry {
    fn internal(&self) -> bool {
        match self {
            TaskEntry::Task(task) => task.internal,
        }
    }

    fn help(&self) -> &String {
        match self {
            TaskEntry::Task(task) => &task.help,
        }
    }

    fn prepare(&self) -> Result<Rc<RefCell<Task>>, WorkshopError> {
        match self {
            TaskEntry::Task(task) => Ok(Rc::from(RefCell::from(task.clone()))),
        }
    }
}

#[macro_export]
/// The task macro allows you to use something like a struct-constructor syntax to create a task. A 'help' property is the only one required. The other available properties are the same as the methods on [Task], and the values are passed to those functions. The `skip_if_older_than` method can be used by wrapping the two arguments in parentheses, like a tuple.
macro_rules! task {
    (@key $task: ident, skip_if_older_than: $value: expr) => {
        $task.skip_if_older_than($value.0,$value.1)
    };
    (@key $task: ident, $key: ident $(: $value: expr)?) => {
        $task.$key($($value)?)
    };
    (help: $help: expr, $($key: ident $(: $value: expr)?),* $(,)?) => {{
        let mut task = $crate::Task::new($help);
        $(
            task!(@key task, $key $(: $value)?);
        )*
        $crate::TaskEntry::Task(task)
    }};
}



#[derive(Default)]
/**
A Workshop object represents a set of tasks to run, and contains functionality for building and running these tasks. 

The easiest way to work with a workshop is by creating a rust-script, or a full-fledged binary project if you want to. In the main function of your script, create the workshop with [Workshop::new], add tasks to it with [Workshop::add] and the [task!] macro, and then call [Workshop::main], which will collect the arguments from the command line.
*/
pub struct Workshop {
    tasks: HashMap<String,TaskEntry>
}

impl Workshop {

    /// Returns a default Workshop, with no tasks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Call this function to add a new task by name.
    pub fn add<Name: Into<String> + Display>(&mut self, name: Name, task: TaskEntry) -> Result<(),WorkshopError> {
        let name = name.into();
        if self.tasks.insert(name.clone(), task).is_some() {
            Err(WorkshopError::TaskAlreadyExists(name))
        } else {
            Ok(())
        }

    }

    /// Call this function to run tasks directly. Specify the tasks to run under `tasks`. If `trace_dependencies` is true, messages will be printed to stdout during dependency calculation. If `trace_commands` is true, messages will be printed to stdout during command processing.
    pub fn run_tasks<TaskName: AsRef<str>>(&mut self, tasks: &[TaskName], trace_dependencies: bool, trace_commands: bool) -> Result<(),WorkshopError> {

        let mut task_list = self.calculate_dependency_list(tasks, trace_dependencies)?;

        for (name,task) in &mut task_list {

            let skip = if let Some(skip) = &task.try_borrow().map_err(|_| WorkshopError::TaskBorrow(name.to_owned()))?.skip {
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

                for command in &mut task.try_borrow_mut().map_err(|_| WorkshopError::TaskBorrowMut(name.to_owned()))?.command {
                    command.run().map_err(|err| WorkshopError::Command(name.clone(), err))?
                }
            } else if trace_commands {
                println!("Skipping task '{name}'.");
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
                    if !value.internal() {
                        let help = &value.help();
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
                if !value.internal() {
                    let help = &value.help();
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
    pub fn calculate_dependency_list<TaskName: AsRef<str>>(&self, tasks: &[TaskName], trace: bool) -> Result<Vec<(String,Rc<RefCell<Task>>)>,WorkshopError> {
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
    marked: HashMap<String,Rc<RefCell<Task>>>,
    cyclical_check: HashMap<String,Rc<RefCell<Task>>>,
    tasks: Vec<(String,Rc<RefCell<Task>>)>,
    trace: bool
}

impl DependencyChecker<'_> {

    /// Creates a new DependencyChecker based on the specified workshop, enabling tracing if `trace` is true.
    pub fn new(workshop: &Workshop, trace: bool) -> DependencyChecker {
        DependencyChecker {
            workshop,
            marked: HashMap::new(),
            cyclical_check: HashMap::new(),
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
        if self.marked.contains_key(node) {
            self.trace(indent,format!("Task {node} already checked."));
            Ok(()) 
        } else if self.cyclical_check.contains_key(node) {
            Err(WorkshopError::CyclicalDependency(node.to_owned()))
        } else if let Some(task_entry) = self.workshop.tasks.get(node) {

            if first_level && task_entry.internal() {

                // this was a task "picked" by the user, but it is marked as internal and therefore not supposed to be called directly.
                Err(WorkshopError::TaskIsNotAvailable(node.to_owned()))

            } else {

                let task = task_entry.prepare()?;

                self.trace(indent,format!("Marking task {node} for cyclical check."));
                self.cyclical_check.insert(node.to_owned(),task.clone());

                for task in &task.try_borrow().map_err(|_| WorkshopError::TaskBorrow(node.to_owned()))?.dependencies {
                    self.visit_dependencies(task,false,&format!("  {indent}"))?;
                }
    
                self.trace(indent,format!("Unmarking task {node} for cyclical check."));
                // it's no longer being checked, so remove it.
                self.cyclical_check.remove(node).expect("This was just inserted, it should still be here.");
    
                self.trace(indent,format!("Marking task {node} as checked."));
                // If I had some sort of map that maintained an order of insert, I wouldn't need a separate marked set and list of tasks. But I feel like adding that sort of crate in would be overkill.
                self.marked.insert(node.to_owned(),task.clone());
    
                self.trace(indent,format!("Adding task {node} to list."));
                // all of its dependencies are already on the list, so this can go on now.
                self.tasks.push((node.to_owned(),task));
    
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
    fn into_tasks(self) -> Vec<(String,Rc<RefCell<Task>>)> {
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
            internal
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

        workshop.add("test-skip-missing", task!{
            help: "Test skipping a missing file",
            skip_if_older_than: (&["src/lib.rs"],&["src/lib.txt"]),
            // If this one causes an error, then the test failed.
        }).expect("Task should have been added.");

        workshop.run_tasks(&["5","8","test-skip-missing"],false,false).expect("Tasks should have run.");

        assert_eq!(result.take(),vec!["2","9","10","11","5","8"]);
        
    }
}
