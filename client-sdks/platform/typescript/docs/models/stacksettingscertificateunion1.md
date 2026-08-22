# StackSettingsCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.

## Supported Types

### `models.StackSettingsCertificateTLSSecretRef1`

```typescript
const value: models.StackSettingsCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.StackSettingsCertificateManagedAcmImport1`

```typescript
const value: models.StackSettingsCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.StackSettingsCertificateAwsAcmArn1`

```typescript
const value: models.StackSettingsCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.StackSettingsCertificateManagedTLSSecret1`

```typescript
const value: models.StackSettingsCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.StackSettingsCertificateNone1`

```typescript
const value: models.StackSettingsCertificateNone1 = {
  mode: "none",
};
```
