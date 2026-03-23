# Config

Type-specific configuration


## Supported Types

### `models.ConfigCli`

```typescript
const value: models.ConfigCli = {
  displayName: "Triston_Hodkiewicz",
  name: "<value>",
  type: "cli",
};
```

### `models.ConfigCloudformation`

```typescript
const value: models.ConfigCloudformation = {
  type: "cloudformation",
};
```

### `models.ConfigHelm`

```typescript
const value: models.ConfigHelm = {
  chartName: "<value>",
  description: "idle ew yippee approach abaft",
  type: "helm",
};
```

### `models.ConfigOperatorImage`

```typescript
const value: models.ConfigOperatorImage = {
  displayName: "Gilda_Gibson",
  name: "<value>",
  type: "operator-image",
};
```

### `models.ConfigTerraform`

```typescript
const value: models.ConfigTerraform = {
  providerName: "<value>",
  resourceType: "<value>",
  type: "terraform",
};
```

