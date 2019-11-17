tftp-server
===========

#### A TFTP server implementation in Rust

[![Build Status](https://travis-ci.org/DarinM223/tftp-server.svg?branch=master)](https://travis-ci.org/DarinM223/tftp-server)

Building and running the server
-------------------------------

In order to build the server, run `cargo build --example server`.

By default, the server will choose a random open port and serve files from the current directory `./`:

```
$ ./target/debug/examples/server
Server created at address: V4(127.0.0.1:61204)
```

In this example, the port number picked was 61204.

You can also specify the port number and directory using the `-p` and `-d` flags.
The `--help` flag shows all available flags that can be used.

```
$ ./target/debug/examples/server -p 61204 -d ./files
Server created at address V4(127.0.0.1:61204)
```

If the port is already taken or there is an error using the port, the server will panic with an IoError.

```
$ ./target/debug/examples/server -p 20
thread 'main' panicked at 'Error creating server: IoError(Os { code: 13, kind: PermissionDenied, message: "Permission denied" })', src/libcore/result.rs:1165:5
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace.
```

You can also run the server with logging enabled. To do this add `RUST_LOG=tftp_server=info` before the command.
For example:

```
$ RUST_LOG=tftp_server=info ./target/debug/examples/server
```

This will run the server with logging enabled so that you can inspect the program's behavior.

Testing
-------

In order to run the tests you can just run `cargo test`. However if you want to show the program's output during the test,
you have to turn on logging. To run tests with logging enabled run:

```
$ RUST_LOG=tftp_server=info cargo test
```
