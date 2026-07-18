# SyncAcquireResponseDeploymentNetworkUnion


## Supported Types

### `models.SyncAcquireResponseDeploymentNetworkUseDefault`

```typescript
const value: models.SyncAcquireResponseDeploymentNetworkUseDefault = {
  type: "use-default",
};
```

### `models.SyncAcquireResponseDeploymentNetworkCreate`

```typescript
const value: models.SyncAcquireResponseDeploymentNetworkCreate = {
  type: "create",
};
```

### `models.SyncAcquireResponseDeploymentNetworkByoVpcAws`

```typescript
const value: models.SyncAcquireResponseDeploymentNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.SyncAcquireResponseDeploymentNetworkByoVpcGcp`

```typescript
const value: models.SyncAcquireResponseDeploymentNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.SyncAcquireResponseDeploymentNetworkByoVnetAzure`

```typescript
const value: models.SyncAcquireResponseDeploymentNetworkByoVnetAzure = {
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

