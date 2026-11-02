from typing import Final

__version__: Final[str]

class SnnAccelerator:
    """SNN layer accelerator (spec v0); attach with ``Emulator.map_device``."""

    def __init__(
        self,
        *,
        max_inputs: int = 1024,
        max_outputs: int = 512,
        weight_bytes: int = 131072,
        t_setup: int = 8,
        t_neuron: int = 1,
        t_syn: int = 1,
        lanes: int = 8,
    ) -> None: ...
    @property
    def size(self) -> int:
        """Size of the address window (1 MiB)."""
    def __mrve32_device__(self) -> object: ...

WINDOW_SIZE: Final[int]
ID: Final[int]
REG_CTRL: Final[int]
REG_STATUS: Final[int]
REG_IRQ_EN: Final[int]
REG_IRQ_STATUS: Final[int]
REG_LAYER_SIZE: Final[int]
REG_NEURON_CFG: Final[int]
REG_CYCLES: Final[int]
REG_IN_SPIKES: Final[int]
REG_OUT_SPIKES: Final[int]
REG_ID: Final[int]
REG_CAPS0: Final[int]
REG_CAPS1: Final[int]
SPIKE_IN_BASE: Final[int]
SPIKE_OUT_BASE: Final[int]
WEIGHTS_BASE: Final[int]
VMEM_BASE: Final[int]
BIAS_BASE: Final[int]
CTRL_START: Final[int]
CTRL_CLEAR_VMEM: Final[int]
STATUS_BUSY: Final[int]
STATUS_DONE: Final[int]
STATUS_ERROR: Final[int]
IRQ_DONE: Final[int]
