# CreateSetupRegistrationOperationRequestManagementConfigUnion

Management configuration for different cloud platforms.

Platform-derived configuration for cross-account/cross-tenant access.
This is NOT user-specified - it's derived from the Manager's ServiceAccount.


## Supported Types

### `models.CreateSetupRegistrationOperationRequestManagementConfigAws`

```typescript
const value: models.CreateSetupRegistrationOperationRequestManagementConfigAws =
  {
    managingRoleArn: "<value>",
    platform: "aws",
  };
```

### `models.CreateSetupRegistrationOperationRequestManagementConfigGcp`

```typescript
const value: models.CreateSetupRegistrationOperationRequestManagementConfigGcp =
  {
    serviceAccountEmail: "<value>",
    platform: "gcp",
  };
```

### `models.CreateSetupRegistrationOperationRequestManagementConfigAzure`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestManagementConfigAzure = {
    managingTenantId: "<id>",
    oidcIssuer: "<value>",
    oidcSubject: "<value>",
    platform: "azure",
  };
```

### `models.CreateSetupRegistrationOperationRequestManagementConfigKubernetes`

```typescript
const value:
  models.CreateSetupRegistrationOperationRequestManagementConfigKubernetes = {
    platform: "kubernetes",
  };
```

### `any`

```typescript
const value: any = "<value>";
```

