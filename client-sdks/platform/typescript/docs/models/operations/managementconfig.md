# ManagementConfig

Management configuration for cross-account access (self-reported via heartbeat)


## Supported Types

### `operations.ManagementConfigAws`

```typescript
const value: operations.ManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `operations.ManagementConfigGcp`

```typescript
const value: operations.ManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `operations.ManagementConfigAzure`

```typescript
const value: operations.ManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `operations.ManagementConfigKubernetes`

```typescript
const value: operations.ManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

