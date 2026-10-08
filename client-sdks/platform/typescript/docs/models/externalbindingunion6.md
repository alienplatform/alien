# ExternalBindingUnion6

Connection details for a Postgres database, one variant per backend.


## Supported Types

### `models.ExternalBindingAurora`

```typescript
const value: models.ExternalBindingAurora = {
  clusterEndpoint: "<value>",
  database: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  passwordSecretArn: null,
  port: "<value>",
  username: null,
  service: "aurora",
  type: "postgres",
};
```

### `models.ExternalBindingCloudSQL`

```typescript
const value: models.ExternalBindingCloudSQL = {
  database: "<value>",
  host: "political-sustenance.biz",
  passwordSecretName: null,
  port: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  serverCaCertificates: null,
  username: "Stephen_Mitchell69",
  service: "cloud-sql",
  type: "postgres",
};
```

### `models.ExternalBindingFlexibleServer`

```typescript
const value: models.ExternalBindingFlexibleServer = {
  database: "<value>",
  host: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  passwordSecretUri: "https://alarmed-unibody.name",
  port: "<value>",
  username: "Trenton98",
  service: "flexible-server",
  type: "postgres",
};
```

### `models.ExternalBindingExternal`

```typescript
const value: models.ExternalBindingExternal = {
  database: "<value>",
  host: "rewarding-premier.biz",
  password: "sxvGxoxE7DCD9Ar",
  port: "<value>",
  username: "Sister_Gutmann",
  service: "external",
  type: "postgres",
};
```

### `models.ExternalBindingLocalPostgres`

```typescript
const value: models.ExternalBindingLocalPostgres = {
  database: "<value>",
  host: "mad-incandescence.net",
  password: "Loz0Nj2fUYzUZVI",
  port: 583955,
  username: "Domenico_Treutel",
  service: "local-postgres",
  type: "postgres",
};
```
