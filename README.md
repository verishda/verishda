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
* `issuer_url`: The issuer URL of the OIDC service to use (tested: [Keycloa](https://www.keycloak.org))

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
```
