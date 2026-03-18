# SyncReconcileResponseNetworkUnion


## Supported Types

### `models.SyncReconcileResponseNetworkUseDefault`

```typescript
const value: models.SyncReconcileResponseNetworkUseDefault = {
  type: "use-default",
};
```

### `models.SyncReconcileResponseNetworkCreate`

```typescript
const value: models.SyncReconcileResponseNetworkCreate = {
  type: "create",
};
```

### `models.SyncReconcileResponseNetworkByoVpcAws`

```typescript
const value: models.SyncReconcileResponseNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.SyncReconcileResponseNetworkByoVpcGcp`

```typescript
const value: models.SyncReconcileResponseNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.SyncReconcileResponseNetworkByoVnetAzure`

```typescript
const value: models.SyncReconcileResponseNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

