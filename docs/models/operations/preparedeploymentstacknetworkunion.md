# PrepareDeploymentStackNetworkUnion


## Supported Types

### `operations.PrepareDeploymentStackNetworkUseDefault`

```typescript
const value: operations.PrepareDeploymentStackNetworkUseDefault = {
  type: "use-default",
};
```

### `operations.PrepareDeploymentStackNetworkCreate`

```typescript
const value: operations.PrepareDeploymentStackNetworkCreate = {
  type: "create",
};
```

### `operations.PrepareDeploymentStackNetworkByoVpcAws`

```typescript
const value: operations.PrepareDeploymentStackNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `operations.PrepareDeploymentStackNetworkByoVpcGcp`

```typescript
const value: operations.PrepareDeploymentStackNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `operations.PrepareDeploymentStackNetworkByoVnetAzure`

```typescript
const value: operations.PrepareDeploymentStackNetworkByoVnetAzure = {
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

