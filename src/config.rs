// Copyright 2018 Johannes M. Griebler
//
// This file is part of mycon.
//
// mycon is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// mycon is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with mycon.  If not, see <https://www.gnu.org/licenses/>.

//! Helper types for storing program configuration.

use std::env;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::Command;

use crate::data::stack::StackStack;
use crate::data::Point;
use crate::data::Value;

/// Specifies how to react when the program tries to access a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileView {
    /// Gives complete access to the real filesystem.
    Real,
    /// Denies any file access. The `i` and `o` instructions will fail and the
    /// interpreter will report that they are unsupported.
    Deny,
}

/// Specifies what action to take when the program attempts to execute a shell
/// command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecAction {
    /// Allows any commands issued by the program to be executed by the system
    /// shell.
    Real,
    /// Denies the ability to execute commands. The `=` instruction will fail
    /// and the interpreter will report that it is unsupported.
    Deny,
}

/// A container for program configuration.
///
/// This includes settings for debug output and how the program interacts with
/// its environment via instructions for I/O and shell command execution.
pub struct Config<'env> {
    trace: bool,
    fmt_trace: Box<dyn FnMut(Trace) + Send + Sync>,
    input: Box<dyn BufRead + 'env + Send + Sync>,
    input_buffer: String,
    output: Box<dyn Write + 'env + Send + Sync>,
    file_view: FileView,
    exec_action: ExecAction,
}

impl<'env> Config<'env> {
    /// Creates a new `Config` with default settings.
    pub fn new() -> Self {
        Config {
            trace: false,
            fmt_trace: Box::new(|trace| {
                eprintln!("{} at {}: {}, {}", trace.id, trace.position, trace.command, trace.stacks);
            }),
            input: Box::new(BufReader::new(io::stdin())),
            input_buffer: String::new(),
            output: Box::new(io::stdout()),
            file_view: FileView::Real,
            exec_action: ExecAction::Real,
        }
    }

    /// Sets whether executed commands should be traced.
    pub fn trace(self, trace: bool) -> Self {
        Self {
            trace,
            ..self
        }
    }

    /// Sets the function to format trace output.
    pub fn trace_format(self, fmt_trace: impl FnMut(Trace) + 'static + Send + Sync) -> Self {
        Self {
            fmt_trace: Box::new(fmt_trace),
            ..self
        }
    }

    /// Sets the input stream of the `Config`.
    pub fn input(self, input: impl BufRead + 'env + Send + Sync) -> Self {
        Self {
            input: Box::new(input),
            ..self
        }
    }

    /// Sets the output stream of the `Config`.
    pub fn output(self, output: impl Write + 'env + Send + Sync) -> Self {
        Self {
            output: Box::new(output),
            ..self
        }
    }

    /// Sets the [`FileView`] of the `Config`.
    ///
    /// [`FileView`]: enum.FileView.html
    pub fn file_view(self, file_view: FileView) -> Self {
        Self {
            file_view,
            ..self
        }
    }

    /// Sets the [`ExecAction`] of the `Config`.
    ///
    /// [`ExecAction`]: enum.ExecAction.html
    pub fn exec_action(self, exec_action: ExecAction) -> Self {
        Self {
            exec_action,
            ..self
        }
    }

    /// Prints the current state of one IP to stderr.
    pub(crate) fn do_trace(&mut self, trace: Trace) {
        if self.trace {
            (self.fmt_trace)(trace);
        }
    }

    /// Tries to write a number to the `Config`'s output stream.
    ///
    /// Returns `true` if it succeeded, `false` otherwise.
    pub(crate) fn write_decimal(&mut self, n: i32) -> bool {
        write!(self.output, "{} ", n).is_ok()
    }

    /// Tries to write a `char` to the `Config`'s output stream.
    ///
    /// Returns `true` if it succeeded, `false` otherwise.
    pub(crate) fn write_char(&mut self, c: char) -> bool {
        write!(self.output, "{}", c).is_ok()
    }

    /// Tries to read a number from the `Config`'s input stream.
    ///
    /// Returns `Some` read number if it succeeded, `None` otherwise.
    pub(crate) fn read_decimal(&mut self) -> Option<i32> {
        if self.output.flush().is_err() {
            return None;
        }

        if self.input_buffer.is_empty() && self.input.read_line(&mut self.input_buffer).is_err() {
            return None;
        }

        let mut found = false;
        let mut ret = 0;
        let mut stop = 0;
        for (i, b) in self.input_buffer.bytes().enumerate() {
            if (b as char).is_digit(10) {
                found = true;
                ret *= 10;
                ret += i32::from(b - b'0');
            } else if found {
                if b == b'\n' {
                    stop = i + 1;
                } else {
                    stop = i;
                }
            }
        }

        self.input_buffer.drain(0..stop);

        // TODO Should this return 0 if no digits were encountered? That seems to be what it's
        // doing right now. Consult the specification about this.
        Some(ret)
    }

    /// Tries to read a `char` from the `Config`'s input stream.
    ///
    /// Returns `Some` read `char` if it succeeded, `None` otherwise.
    pub(crate) fn read_char(&mut self) -> Option<char> {
        if self.output.flush().is_err() {
            return None;
        }

        if self.input_buffer.is_empty() && self.input.read_line(&mut self.input_buffer).is_err() {
            return None;
        }

        let c = self.input_buffer.chars().nth(0).unwrap();
        let mut stop = 1;

        while !self.input_buffer.is_char_boundary(stop) {
            stop += 1;
        }

        self.input_buffer.drain(0..stop);

        Some(c)
    }

    /// Tries to write the given string to a file.
    ///
    /// Returns `true` if it succeeded, `false` otherwise.
    pub(crate) fn write_file(&self, path: &str, data: &str) -> bool {
        match self.file_view {
            FileView::Real => (),
            FileView::Deny  => return false,
        }

        let mut f = match File::create(path) {
            Ok(f)  => f,
            Err(_) => return false,
        };

        f.write_all(data.as_bytes()).is_ok()
    }

    /// Tries to read from a file.
    ///
    /// Returns `Some` read string, or `None` if it failed.
    pub(crate) fn read_file(&self, path: &str) -> Option<String> {
        match self.file_view {
            FileView::Real => (),
            FileView::Deny  => return None,
        }

        let mut f = match File::open(path) {
            Ok(f)  => f,
            Err(_) => return None,
        };

        let mut s = String::new();

        if f.read_to_string(&mut s).is_err() {
            None
        } else {
            Some(s)
        }
    }

    /// Takes a string and tries to execute it with `sh`.
    ///
    /// Returns `Some` [`Value`] with `sh`'s exit code if it was able to obtain
    /// it, and `None` otherwise.
    ///
    /// If a [`Value`] is returned, the exit code can (in general) not be used
    /// to determine whether an error was raised by `sh` trying to execute the
    /// given command, or by the command itself.
    ///
    /// Also, a return of `None` can mean that the attempt to execute `sh`
    /// failed, that `sh` was terminated by a signal or that this
    /// `Config`'s settings don't allow command execution.
    ///
    /// [`Value`]: ../../data/type.Value.html
    pub(crate) fn execute(&self, cmd: &str) -> Option<Value> {
        if self.exec_action != ExecAction::Deny {
            match Command::new("sh").args(&["-c", cmd]).status() {
                Ok(st) => st.code(),
                Err(_) => None,
            }
        } else {
            None
        }
    }

    /// Returns flags containing information about functionality available to
    /// the program.
    ///
    /// The flags are in the format returned by the `y` instruction to a running
    /// Befunge-98 program.
    pub(crate) fn flags(&self) -> Value {
        // 't' is always supported.
        // TODO Should this be configurable?
        let mut flags = 1;

        if self.file_view != FileView::Deny {
            // 'i' and 'o' are supported.
            flags |= 0x6;
        }

        if self.exec_action != ExecAction::Deny {
            // '=' is supported.
            flags |= 0x8;
        }

        flags
    }

    /// Returns a value indicating the behavior of the `=` instruction.
    pub(crate) fn operating_paradigm(&self) -> Value {
        if self.exec_action != ExecAction::Deny {
            1
        } else {
            0
        }
    }

    /// Returns an iterator over the command-line arguments of the program.
    pub(crate) fn cmd_args(&self) -> impl Iterator<Item = String> {
        env::args().rev()
    }

    /// Returns an iterator over the environment variables.
    pub(crate) fn env_vars(&self) -> impl Iterator<Item = (String, String)> {
        env::vars()
    }
}

/// Values available to trace output.
pub struct Trace<'a> {
    id: Value,
    command: char,
    position: Point,
    stacks: &'a StackStack,
}

impl<'a> Trace<'a> {
    pub(crate) fn new(id: i32, command: char, position: Point, stacks: &'a StackStack) -> Self {
        Self {
            id,
            command,
            position,
            stacks,
        }
    }

    /// Returns the ID of the IP that executed a command.
    pub fn id(&self) -> String {
        self.id.to_string()
    }

    /// Returns the command that was executed.
    pub fn command(&self) -> String {
        self.command.to_string()
    }

    /// Returns the position at which the command was encountered.
    pub fn position(&self) -> String {
        self.position.to_string()
    }

    /// Returns the stacks of the IP after the command was executed.
    pub fn stacks(&self) -> String {
        self.stacks.to_string()
    }
}
