tftp-server
===========

#### A TFTP server implementation in Rust

Building and running the server
-------------------------------

To build the server, simply run `cargo build`. Once compiled you can run the server without arguments, in which case it will pick a random port on the loopback address and serve from the current directory:

```
$ ./target/debug/tftp_server_bin
Server created at address: V4(127.0.0.1:61204)
```

In this example, the port number picked was 61204.

You can also explicitly specify the address (and optionally the port) on which it will listen

```
$ ./target/debug/tftp_server_bin --ipv4 192.168.0.54
Server created at address: V4(192.168.0.54:43604)
```

or

```
$ ./target/debug/tftp_server_bin --ipv4 192.168.0.54:35000
Server created at address: V4(192.168.0.54:35000)
```

If the server cannot bind to the given address:port (or if it cannot find a random port for the address) then it will panic with an IoError.
```
$ ./target/debug/tftp_server_bin --ipv4 127.0.0.1:20
thread 'main' panicked at 'Error creating server: IoError(Error { repr: Os { code: 13, message: "Permission denied" } })', ../src/libcore/result.rs:799
note: Run with `RUST_BACKTRACE=1` for a backtrace.
```


Features
--------
All features are implemented in the library. The binary target is a only an argument-parsing thin wrapper over it for direct usage conveninence.

Available features:
* `-4` or `--ipv4` to specify address:port to listen on (currently only a single IPv4 one)
* `-r` will make the server treat the served directory as read-only (it will reject all write requests)
* see TODO section below


Logging and Testing
-------------------

You can also run the server with logging enabled. To do this add `RUST_LOG=tftp_server=info` before the command.
For example:

```
$ RUST_LOG=tftp_server=info ./target/debug/tftp_server_bin
```

This will run the server with logging enabled so that you can inspect the program's behavior.

To run the tests you can just run `cargo test`. However if you want to show the program's output during the test,
you have to turn on logging. To run tests with logging enabled run:

```
$ RUST_LOG=tftp_server=info cargo test
```

Feature TODOs
-----

* [ ] serve from specified directory, not just the current one
* [x] treat directory as readonly (reject write requests)
* [ ] IPv6 support (and thus multiple address support)
* [ ] CLI switches for logging
