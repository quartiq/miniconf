MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 256K
  RAM : ORIGIN = 0x20000000, LENGTH = 64K
}

SECTIONS {
  linkme_REGISTRY_KV : { *(linkme_REGISTRY_KV) } > FLASH
  linkm2_REGISTRY_KV : { *(linkm2_REGISTRY_KV) } > FLASH
}
INSERT AFTER .rodata
