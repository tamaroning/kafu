apiVersion: v1
kind: ConfigMap
metadata:
  name: {configmap_name}
  labels:
    kafu-service-name: {config_name}
{{ if instance_id }}    kafu-instance: {instance_id}
{{ endif }}data:
  kafu-config.yaml: |
{config_content|unescaped}
