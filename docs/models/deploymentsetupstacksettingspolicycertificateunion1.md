# DeploymentSetupStackSettingsPolicyCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentSetupStackSettingsPolicyCertificateTLSSecretRef1`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyCertificateTLSSecretRef1 =
  {
    secretName: "<value>",
    mode: "tlsSecretRef",
  };
```

### `models.DeploymentSetupStackSettingsPolicyCertificateManagedAcmImport1`

```typescript
const value:
  models.DeploymentSetupStackSettingsPolicyCertificateManagedAcmImport1 = {
    mode: "managedAcmImport",
  };
```

### `models.DeploymentSetupStackSettingsPolicyCertificateAwsAcmArn1`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentSetupStackSettingsPolicyCertificateManagedTLSSecret1`

```typescript
const value:
  models.DeploymentSetupStackSettingsPolicyCertificateManagedTLSSecret1 = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.DeploymentSetupStackSettingsPolicyCertificateNone1`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyCertificateNone1 = {
  mode: "none",
};
```

