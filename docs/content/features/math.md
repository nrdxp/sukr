+++
title = "Math Rendering"
description = "Build-time LaTeX math to MathML"
weight = 5
+++

sukr renders LaTeX math expressions at build time to MathML. The output is browser-native — no client-side JavaScript required.

## Inline Math

Use single dollar signs for inline math:

```markdown
The quadratic formula is $x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}$.
```

Renders as: The quadratic formula is $x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}$.

## Display Math

Use double dollar signs for display (block) math:

```markdown
$$
E = mc^2
$$
```

Or fence with `math` language:

````markdown
```math
\int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
```
````

## Supported Features

pulldown-latex supports a broad subset of LaTeX math:

| Feature                 | Syntax                                         | Rendered                                       |
| ----------------------- | ---------------------------------------------- | ---------------------------------------------- |
| Greek letters           | `\alpha, \beta, \gamma`                        | $\alpha, \beta, \gamma$                        |
| Fractions               | `\frac{a}{b}`                                  | $\frac{a}{b}$                                  |
| Subscripts/superscripts | `x_i^2`                                        | $x_i^2$                                        |
| Summations              | `\sum_{i=1}^{n} i`                             | $\sum_{i=1}^{n} i$                             |
| Integrals               | `\int_a^b f(x)\,dx`                            | $\int_a^b f(x)\,dx$                            |
| Square roots            | `\sqrt{x^2 + y^2}`                             | $\sqrt{x^2 + y^2}$                             |
| Matrices                | `\begin{pmatrix} a & b \\ c & d \end{pmatrix}` | $\begin{pmatrix} a & b \\ c & d \end{pmatrix}$ |

### Display Math Examples

The Gaussian integral:

$$\int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi}$$

Euler's identity:

$$e^{i\pi} + 1 = 0$$

The Schrödinger equation:

$$i\hbar\frac{\partial}{\partial t}\Psi = \hat{H}\Psi$$

## How It Works

1. Math delimiters (`$...$`, `$$...$$`) are detected during parsing
2. pulldown-latex converts the expression to MathML
3. Browsers render MathML natively — no fonts or JavaScript needed
4. Output is pure HTML with embedded `<math>` elements

## Error Handling

Invalid LaTeX math syntax triggers a compilation error and halts the build. This ensures that any rendered formula on the published site is syntactically valid.

If the parser encounters malformed LaTeX, `sukr` outputs the error message and the path to the console:

```text
render error: math render error in `\invalid{command}`: ...
```
