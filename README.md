# verishda server

## build

The server is built using [spin](https://developer.fermyon.com/spin), Fermyon's wasm runtime.

Install spin as [described here](https://developer.fermyon.com/spin/install#installing-spin), then run:

```bash
spin build
```

## deploy

to run locally, use
```bash
spin up
```

However, if you want to use [fermyon cloud](https://cloud.fermyon.com/) (free for small workloads), you'll need to get an account there first. Then, use

```
spin deploy
```

## configure

The verishda server is configured using [spin Application variables](https://developer.fermyon.com/spin/variables). 

We need the following configuration variables:
* `pg_address`: the URL to reach the Postgres database. Note that the database must have beein initialized with the `create_tables.sql` script in the root dir. 
* `issuer_url`: The issuer URL of the OpenID service to use (tested: [Keycloak](https://www.keycloak.org))
* `rust_log`: Optional logging configuration. If provided, contains a string describing the logging settings. See the [`env_logger` create documenation](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for details.

In order to use the `swagger-ui` built into the server, you'll need to create a client (for Keycloak, this is described [here](https://www.keycloak.org/docs/latest/server_admin/index.html#_oidc_clients)). In this guide, we'll assume you called the client 'swagger' (but you can call it anything you want). Also make sure that you'll add `<base-url>/api/public/*` as a redirection URL. So if you're running the server on `localhost:3000` for local development with spin, the redirect URL is `http://localhost:3000/api/public/*`.
In Fermyon Cloud, this could for instance be `https://verishda.fermyon.app:3000/api/public/*`

### Fermyon Cloud
To set these variables, you'll need to run shell commands like these:

```sh
# the app is names 'verishda-server' in our spin.toml

# set the database
spin cloud variables set --app verishda-server pg_address="postgres://user:password@host/dbname"
# set the issuer url, pointing at a keycloak instance - this is used to fetch more config via OIDC discovery
spin cloud variables set --app verishda-server issuer_url="https://mykeycloak/auth/realms/myrealm" 
```

### Local Development
When running spin locally, spin can read the variables from a `.env` file as environment variables. Even though spin variables are _not_ environment variables, there is a mapping from envrionment variables to spin variables such that an environment variables names starting with `SPIN_CONFIG_` will be stripped of the prefix and lowercased and is then visible under that name as a spin variable, so `SPIN_CONFIG_FOO` is then visible to the application as spin variable `foo`.

In essence, create a .env file like this:
```sh
# replace '...' with OpenID Connect issuer URL to allow of OpenID Connect Discovery
SPIN_CONFIG_PG_ADDRESS=...

# replace '...' with Postgres URL, which must also include the credentials
SPIN_CONFIG_ISSUER_URL=...

# if enabled (remove comment), the format for the logging config is defined here:
# https://docs.rs/env_logger/latest/env_logger/#enabling-logging
#SPIN_RUST_LOG=...
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
