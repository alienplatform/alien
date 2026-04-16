# ManagerHeartbeatRequestManagementConfigUnion

Management configuration for cross-account access (from ServiceAccount binding)


## Supported Types

### `models.ManagerHeartbeatRequestManagementConfigAws`

```typescript
const value: models.ManagerHeartbeatRequestManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

### `models.ManagerHeartbeatRequestManagementConfigGcp`

```typescript
const value: models.ManagerHeartbeatRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

### `models.ManagerHeartbeatRequestManagementConfigAzure`

```typescript
const value: models.ManagerHeartbeatRequestManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

### `models.ManagerHeartbeatRequestManagementConfigKubernetes`

```typescript
const value: models.ManagerHeartbeatRequestManagementConfigKubernetes = {
  platform: "kubernetes",
};
```

### `any`

```typescript
const value: any = "<value>";
```

