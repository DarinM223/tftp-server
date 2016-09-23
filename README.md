tftp-server
===========

#### A TFTP server implementation in Rust

[![Build Status](https://travis-ci.org/DarinM223/tftp-server.svg?branch=master)](https://travis-ci.org/DarinM223/tftp-server)

Building and running the server
-------------------------------

In order to build the server, simply run `cargo build`. Then once the server has been compiled you can run the server using two different ways.

The first way allows you to run the server without specifying a port. The server will find an open port to run itself on and inform you of the port it picked.

```
$ ./target/debug/tftp_server_bin
Server created at address: V4(127.0.0.1:61204)
Getting None when receiving from server socket
```

In this example, the port number picked was 61204.

The second way allows you to choose an open port for the server to run on. You specify the port number as a command line argument when running the server.

```
$ ./target/debug/tftp_server_bin 61204
Getting None when receiving from server socket
```

If the port is already taken or there is an error using the port, the server will panic with an IoError.

```
$ ./target/debug/tftp_server_bin 20
thread 'main' panicked at 'Error creating server: IoError(Error { repr: Os { code: 13, message: "Permission denied" } })', ../src/libcore/result.rs:799
note: Run with `RUST_BACKTRACE=1` for a backtrace.
```

You can also run the server with logging enabled. To do this add `RUST_LOG=tftp_server=info` before the command.
For example:

```
$ RUST_LOG=tftp_server=info ./target/debug/tftp_server_bin
```

This will run the server with logging enabled so that you can inspect the program's behavior.

Testing
-------

In order to run the tests you can just run `cargo test`. However if you want to show the program's output during the test,
you have to turn on logging. To run tests with logging enabled run:

```
$ RUST_LOG=tftp_server=info cargo test
```
