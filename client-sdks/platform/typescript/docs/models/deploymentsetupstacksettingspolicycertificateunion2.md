# DeploymentSetupStackSettingsPolicyCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentSetupStackSettingsPolicyCertificateTLSSecretRef2`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyCertificateTLSSecretRef2 =
  {
    secretName: "<value>",
    mode: "tlsSecretRef",
  };
```

### `models.DeploymentSetupStackSettingsPolicyCertificateManagedAcmImport2`

```typescript
const value:
  models.DeploymentSetupStackSettingsPolicyCertificateManagedAcmImport2 = {
    mode: "managedAcmImport",
  };
```

### `models.DeploymentSetupStackSettingsPolicyCertificateAwsAcmArn2`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentSetupStackSettingsPolicyCertificateManagedTLSSecret2`

```typescript
const value:
  models.DeploymentSetupStackSettingsPolicyCertificateManagedTLSSecret2 = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.DeploymentSetupStackSettingsPolicyCertificateNone2`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyCertificateNone2 = {
  mode: "none",
};
```

