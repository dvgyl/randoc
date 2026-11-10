# Part 1: the pump station

The station moves water from the *lower tank* to the **upper tank**. Read the
[operating limits][limits-1] before you start a pump. Each pump has a
`status` word and a counter.[^count-1]

## Start procedure 1

1. Open the inlet valve.
2. Set the selector to `AUTO`.
   - Make sure that the green lamp comes on.
   - If the red lamp comes on, stop and read the fault list.
3. Push the start button.

## Equipment list 1

| Item | Part number | Quantity | Note |
|:-----|------------:|:--------:|------|
| Pump | P-100-1 | 2 | Duty and standby |
| Valve | V-220 | 6 | Ball type |
| Sensor | S-31 | 4 | Level, 4 to 20 mA |

Term 1
:   A word that the station manual uses with one meaning.

> Do not run a pump with the outlet valve closed. The pump becomes hot and
> the seal can fail.

```python
def flow(level, area):
    """Return the flow in cubic metres for each hour."""
    return level * area * 3600
```

See <https://example.com/station/1> for the full manual, and the
[parts catalogue](https://example.com/parts "Parts") for spare parts.

[limits-1]: https://example.com/limits/1 "Operating limits"
[^count-1]: The counter shows the number of starts since the last service.

