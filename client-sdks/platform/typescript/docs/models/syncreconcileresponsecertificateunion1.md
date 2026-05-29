# SyncReconcileResponseCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncReconcileResponseCertificateTLSSecretRef1`

```typescript
const value: models.SyncReconcileResponseCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncReconcileResponseCertificateManagedAcmImport1`

```typescript
const value: models.SyncReconcileResponseCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.SyncReconcileResponseCertificateAwsAcmArn1`

```typescript
const value: models.SyncReconcileResponseCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncReconcileResponseCertificateManagedTLSSecret1`

```typescript
const value: models.SyncReconcileResponseCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.SyncReconcileResponseCertificateNone1`

```typescript
const value: models.SyncReconcileResponseCertificateNone1 = {
  mode: "none",
};
```

