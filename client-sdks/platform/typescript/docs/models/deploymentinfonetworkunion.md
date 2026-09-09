# DeploymentInfoNetworkUnion


## Supported Types

### `models.DeploymentInfoNetworkUseDefault`

```typescript
const value: models.DeploymentInfoNetworkUseDefault = {
  type: "use-default",
};
```

### `models.DeploymentInfoNetworkCreate`

```typescript
const value: models.DeploymentInfoNetworkCreate = {
  type: "create",
};
```

### `models.DeploymentInfoNetworkByoVpcAws`

```typescript
const value: models.DeploymentInfoNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
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

### `models.DeploymentInfoNetworkByoVpcGcp`

```typescript
const value: models.DeploymentInfoNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.DeploymentInfoNetworkByoVnetAzure`

```typescript
const value: models.DeploymentInfoNetworkByoVnetAzure = {
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

