# verishda server

## build

This server supports two modes and therefore builds two binaries:
* `verishda`: an application designed to run in the (Shuttle)[https://shuttle.rs]  hosting environment
* `verishda-standalone`: a standalone server application, suitable for running without shuttle



## Deploy on Shuttle

You need to signup to shuttle and create a project (here)[https://console.shuttle.rs/login?from=%2F] (Github account required) and deploy it using
```bash
cargo shuttle project deploy
```

Beware: you need to configure your server first!

## Configure

The verishda server is configured using environment variables (or Shuttle secrets, which work the same way)

We need the following configuration variables:
* `PG_ADDRESS`: the URL to reach the Postgres database. Note that the database must have beein initialized with the `create_tables.sql` script in the root directory. Not used when deployed in Shuttle, as they provide the DB connection directly - otherwise REQUIRED.
* `ISSUER_URL`: The issuer URL of the OpenID service to use (tested: [Keycloak](https://www.keycloak.org)). The issuer URL can be found in the `.well-known` auto-config URL that OpenID identity servers provide. REQUIRED.
* `RUST_LOG`: Logging configuration. If provided, contains a string describing the logging settings. See the [`env_logger` create documenation](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for details. OPTIONAL
* `FORWARDED_PROTO`: When configured behind a reverse proxy that terminates TLS, this option can override the calling URI scheme detection. Not needed if the reverse proxy sets the `X-Forwarded-Proto` header. When deploying to Shuttle hosting, set to `https` (but don't set it when testing the shuttle app locally).

In order to use the `swagger-ui` built into the server, you'll need to create a client (for Keycloak, this is described [here](https://www.keycloak.org/docs/latest/server_admin/index.html#_oidc_clients)). In this guide, we'll assume you called the client 'swagger' (but you can call it anything you want). Also make sure that you'll add `<base-url>/api/public/*` as a redirection URL. So if you're running the server on `localhost:3000` for local development, the redirect URL is `http://localhost:3000/api/public/*`.
In Shuttle, this could for instance be `https://verishda.shuttleapp.rs/api/public/*`

### Shuttle Configuration
Before deploying to shuttle (see above), create a Shuttle secrets file:
* `Secrets.dev.toml` contains the config variables for local development
* `Secrets.toml` for the config variables used when deploying to Shuttle's cloud offering

The config variables are simply written into the file. Remember to use quotes for the values:
```
ISSUER_URL='https://path.to/identity-server'
FORWARDED_PROTO='https'
```

### Configuration for standalone apps
When running standalone, the server can read the variables from a `.env` file as environment variables. 

In essence, create a .env file like this:
```sh
# replace '...' with OpenID Connect issuer URL to allow of OpenID Connect Discovery
ISSUER_URL=...

# replace '...' with Postgres URL, which must also include the credentials
PG_ADDRESS=...

# if enabled (remove comment), the format for the logging config is defined here:
# https://docs.rs/env_logger/latest/env_logger/#enabling-logging
#RUST_LOG=...
```

## Try in Swagger-UI

The server comes with it's own swagger UI. To use it, point your browser to [`http://localhost:3000/api`](http://localhost:3000/api) if you're running locally or e.g. [`https://verishda.fermyon.app/api`](https://verishda.fermyon.app/api). 

* Find the 'Authorize'-Button and click it
* Enter 'swagger' in the field 'client-id' (or whatever client name you assigned in your OpenID Connect service)
* Click 'login' - this should direct you to the login screen, where you login
* You should now see the status dialog, where you can click 'Close'.

All services should be callable now via swagger-ui.

### Troubleshooting Swagger-UI

* When using Keycloak, make sure you are *NOT* using the admin user when accessing the server, because by default that user will come with the wrong audience (`aud` claim), and you'll get HTTP Status `401`.
* You may also get `401` if you wait too long after login, because swagger does not perform a token refresh, apparently, and access tokens usually expire quickly (typically 5min). So simply log out and back in again.
