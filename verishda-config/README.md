# Verishda Configuration

The verishda system uses a number of configuration variables, of which some can be shared across client and server. This is very useful for local development:
When using a `.env` file in the project root directory, the subprojects `verishda-server` and `verishda-slint` will use the `.env` file from the parent directory and read the shared variables from there. Because client and server need to agree on the configuration, this prevents errors where the same variable is configured differently for client and server.

| variable | description | relevant for server (S) / client (C) |
| -------- | ----------- | -------------------------------------|
| `PG_ADDRESS` | the URL to reach the Postgres database. Not used when deployed in Shuttle, as they provide the DB connection directly - otherwise REQUIRED. | S |
| `ISSUER_URL` | The issuer URL of the OpenID service to use (tested: [Keycloak](https://www.keycloak.org)). The issuer URL can be found in the `.well-known` auto-config URL that OpenID identity servers provide. OPTIONAL. | S,C |
| `RUST_LOG` | Logging configuration. If provided, contains a string describing the logging settings. See the [`env_logger` create documenation](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for details. OPTIONAL | S, C |
| `FORWARDED_PROTO` | When configured behind a reverse proxy that terminates TLS, this option can override the calling URI scheme detection. Not needed if the reverse proxy sets the `X-Forwarded-Proto` header. When deploying to Shuttle hosting, set to `https` (but don't set it when testing the shuttle app locally).| S |
| `API_BASE_URL` | The URL where to find the verishda server | C |

If an optional variable is not provided, it will default to a value built into the default configuration (these are the public verishda URLs used in production hosting).


