# SyncReconcileResponseCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncReconcileResponseCertificateTLSSecretRef2`

```typescript
const value: models.SyncReconcileResponseCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncReconcileResponseCertificateManagedAcmImport2`

```typescript
const value: models.SyncReconcileResponseCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.SyncReconcileResponseCertificateAwsAcmArn2`

```typescript
const value: models.SyncReconcileResponseCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncReconcileResponseCertificateManagedTLSSecret2`

```typescript
const value: models.SyncReconcileResponseCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.SyncReconcileResponseCertificateNone2`

```typescript
const value: models.SyncReconcileResponseCertificateNone2 = {
  mode: "none",
};
```

