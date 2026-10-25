# Chat: Gibbs energy of ions

## User

What is the **electrochemical potential** of an ion? Please use $\tilde{\mu}$.

## Assistant

<details>
<summary>Thinking</summary>

The user wants the definition. Keep it short.

</details>

For an ion:

$$
\tilde{\mu}
=
\mu^0 + RT\ln a + zF\phi
$$

> [!NOTE]
> $\phi$ is the local electric potential.

```python
def mu(a, z, phi, R=8.314, T=298.15, F=96485):
    return R * T * math.log(a) + z * F * phi
```

```mermaid
graph LR
  A[Gradient] --> B[Work]
```

```svg
<svg viewBox="0 0 100 40" xmlns="http://www.w3.org/2000/svg"><rect width="100" height="40" fill="#eef"/><circle cx="50" cy="20" r="14" fill="tomato"/></svg>
```

| Symbol | Meaning |
|---|---|
| $\mu^0$ | standard chemical potential |

**You:** And for a membrane?

**Claude:** The Nernst equation follows from $\Delta\tilde{\mu}=0$:

\begin{align}
E &= \frac{RT}{zF}\ln\frac{c_o}{c_i} \\
  &\approx 61.5\,\text{mV}\,\log_{10}\frac{c_o}{c_i}
\end{align}

## You said:

Thanks!
