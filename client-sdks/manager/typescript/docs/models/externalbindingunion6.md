# ExternalBindingUnion6

External Postgres binding (operator-provided / BYO database)


## Supported Types

### `models.ExternalBindingAurora`

```typescript
const value: models.ExternalBindingAurora = {
  clusterEndpoint: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  database: "<value>",
  passwordSecretArn: "<value>",
  port: "<value>",
  username: "Isai.Beatty57",
  service: "aurora",
  type: "postgres",
};
```

### `models.ExternalBindingAuroraManaged`

```typescript
const value: models.ExternalBindingAuroraManaged = {
  clusterEndpoint: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  database: "<value>",
  passwordSecretArn: "<value>",
  port: "<value>",
  username: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "aurora-managed",
  type: "postgres",
};
```

### `models.ExternalBindingCloudSQL`

```typescript
const value: models.ExternalBindingCloudSQL = {
  database: "<value>",
  host: "understated-valuable.name",
  passwordSecretName: "<value>",
  port: 23027,
  serverCaCertificates: "<value>",
  username: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "cloud-sql",
  type: "postgres",
};
```

### `models.ExternalBindingFlexibleServer`

```typescript
const value: models.ExternalBindingFlexibleServer = {
  database: "<value>",
  host: "trusty-peninsula.biz",
  passwordSecretUri: "https://alarmed-unibody.name",
  port: "<value>",
  username: "Shemar_Wehner",
  service: "flexible-server",
  type: "postgres",
};
```

### `models.ExternalBindingExternal`

```typescript
const value: models.ExternalBindingExternal = {
  database: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  host: "yellow-handover.net",
  password: {
    secretKeyRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  port: 106185,
  username: "Wiley_Ortiz",
  service: "external",
  type: "postgres",
};
```

### `models.ExternalBindingLocalPostgres`

```typescript
const value: models.ExternalBindingLocalPostgres = {
  database: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  host: "unruly-hubris.info",
  password: "LkLoz0Nj2fUYzUZ",
  port: 273248,
  username: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local-postgres",
  type: "postgres",
};
```
