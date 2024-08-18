# Verishda client

This client is based on [Slint](https://slint.dev) and currently is the only functional client for the [Verishda server](../verishda-server/README.md).

The client is currently focused on Windows and MacOS desktops. (Linux works in theory, but without support for getting the current location)

## Building

On Windows, you may need Visual Studio tooling installed.

On Mac OS, the app will not be able to request authorization for getting the current geolocation fromthe system unless it is delivered and started as an app bundle. Therefore, use [`cargo bundle`](https://github.com/burtonageo/cargo-bundle) to package it. It can be executed also directly using `cargo run`, but geolocation tracking will not work in this case.