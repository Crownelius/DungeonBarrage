using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// Shared code-drawn presentation primitives for the retro-arcade shell. These helpers own visual
/// semantics only; screen flow and every gameplay decision remain outside this type.
/// </summary>
internal static class RetroArcadeUi
{
    internal static readonly Color Void = new("#090b16");
    internal static readonly Color DeepNavy = new("#101526");
    internal static readonly Color Panel = new("#151b2d");
    internal static readonly Color PanelRaised = new("#202941");
    internal static readonly Color Ink = new("#f5f1dc");
    internal static readonly Color MutedInk = new("#9aa4bc");
    internal static readonly Color Ember = new("#f04b2f");
    internal static readonly Color EmberDark = new("#7a1d24");
    internal static readonly Color Coin = new("#ffc43d");
    internal static readonly Color Cyan = new("#35e2f2");
    internal static readonly Color Purple = new("#b96cff");
    internal static readonly Color Success = new("#58e37d");
    internal static readonly Color Locked = new("#596176");

    internal static void DrawBackdrop(CanvasItem canvas, Vector2 size, ulong visualTimeMsec, bool reduceMotion)
    {
        ArgumentNullException.ThrowIfNull(canvas);

        canvas.DrawRect(new Rect2(Vector2.Zero, size), Void);

        const int bands = 12;
        for (var band = 0; band < bands; band++)
        {
            var ratio = band / (float)bands;
            var color = DeepNavy.Lerp(Void, ratio * 0.72f);
            canvas.DrawRect(
                new Rect2(0f, band * size.Y / bands, size.X, (size.Y / bands) + 1f),
                color);
        }

        var pulse = reduceMotion ? 0.42f : 0.34f + (Mathf.Sin(visualTimeMsec / 620f) * 0.08f);
        canvas.DrawCircle(new Vector2(size.X * 0.78f, size.Y * 0.25f), size.Y * 0.31f, new Color(0.05f, 0.55f, 0.66f, pulse * 0.18f));
        canvas.DrawCircle(new Vector2(size.X * 0.16f, size.Y * 0.62f), size.Y * 0.28f, new Color(0.80f, 0.14f, 0.08f, pulse * 0.12f));

        DrawStoneBorder(canvas, size);

        for (var y = 2f; y < size.Y; y += 6f)
        {
            canvas.DrawLine(new Vector2(0f, y), new Vector2(size.X, y), new Color(0f, 0f, 0f, 0.10f), 1f);
        }

        var sparkShift = reduceMotion ? 0f : visualTimeMsec / 55f;
        for (var index = 0; index < 18; index++)
        {
            var x = (index * 173f + 71f) % Math.Max(size.X, 1f);
            var y = (index * 97f + sparkShift) % Math.Max(size.Y, 1f);
            var color = index % 3 == 0 ? Cyan : Coin;
            canvas.DrawRect(new Rect2(x, y, 3f, 3f), new Color(color, 0.45f));
        }
    }

    internal static void DrawPanel(CanvasItem canvas, Rect2 rect, Color accent, bool selected = false)
    {
        ArgumentNullException.ThrowIfNull(canvas);

        canvas.DrawRect(new Rect2(rect.Position + new Vector2(8f, 9f), rect.Size), new Color(0f, 0f, 0f, 0.48f));
        canvas.DrawRect(rect, selected ? PanelRaised : Panel);
        canvas.DrawRect(rect, new Color(accent, selected ? 0.98f : 0.62f), filled: false, width: selected ? 4f : 2f);
        canvas.DrawRect(rect.Grow(-7f), new Color(accent, selected ? 0.34f : 0.15f), filled: false, width: 1f);
        DrawPixelCorners(canvas, rect, accent, selected ? 13f : 9f);
    }

    internal static void DrawButton(
        CanvasItem canvas,
        Font font,
        Rect2 rect,
        string label,
        Color accent,
        bool selected,
        bool locked = false,
        int fontSize = 22)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(font);

        var effectiveAccent = locked ? Locked : accent;
        DrawPanel(canvas, rect, effectiveAccent, selected && !locked);
        if (selected && !locked)
        {
            canvas.DrawRect(rect.Grow(-11f), new Color(effectiveAccent, 0.13f));
        }

        DrawCenteredText(canvas, font, rect, label, fontSize, locked ? MutedInk : Ink);
        if (locked)
        {
            DrawStatusPill(canvas, font, new Vector2(rect.End.X - 80f, rect.Position.Y + 17f), "LOCKED", Locked);
        }
    }

    internal static void DrawStatusPill(CanvasItem canvas, Font font, Vector2 position, string text, Color color)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(font);

        var textSize = font.GetStringSize(text, fontSize: 11);
        var rect = new Rect2(position, textSize + new Vector2(18f, 10f));
        canvas.DrawRect(rect, new Color(color, 0.22f));
        canvas.DrawRect(rect, new Color(color, 0.86f), filled: false, width: 1f);
        canvas.DrawString(font, rect.Position + new Vector2(9f, 15f), text, fontSize: 11, modulate: color);
    }

    internal static void DrawScreenHeader(CanvasItem canvas, Font font, Vector2 viewport, string eyebrow, string title, string subtitle)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(font);

        var header = new Rect2(34f, 26f, viewport.X - 68f, 92f);
        DrawPanel(canvas, header, Ember);
        canvas.DrawRect(new Rect2(header.Position, new Vector2(8f, header.Size.Y)), Ember);
        canvas.DrawString(font, header.Position + new Vector2(28f, 25f), eyebrow, fontSize: 12, modulate: Cyan);
        DrawShadowText(canvas, font, header.Position + new Vector2(28f, 62f), title, 30, Coin);
        var subtitleSize = font.GetStringSize(subtitle, fontSize: 13);
        canvas.DrawString(
            font,
            new Vector2(header.End.X - subtitleSize.X - 24f, header.Position.Y + 57f),
            subtitle,
            fontSize: 13,
            modulate: MutedInk);
    }

    internal static void DrawShadowText(CanvasItem canvas, Font font, Vector2 position, string text, int fontSize, Color color)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(font);

        canvas.DrawString(font, position + new Vector2(4f, 4f), text, fontSize: fontSize, modulate: new Color(0f, 0f, 0f, 0.78f));
        canvas.DrawString(font, position, text, fontSize: fontSize, modulate: color);
    }

    internal static void DrawCenteredText(CanvasItem canvas, Font font, Rect2 rect, string text, int fontSize, Color color)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(font);

        var size = font.GetStringSize(text, fontSize: fontSize);
        var position = rect.Position + new Vector2(
            (rect.Size.X - size.X) * 0.5f,
            ((rect.Size.Y - size.Y) * 0.5f) + size.Y);
        canvas.DrawString(font, position, text, fontSize: fontSize, modulate: color);
    }

    private static void DrawStoneBorder(CanvasItem canvas, Vector2 size)
    {
        const float depth = 15f;
        canvas.DrawRect(new Rect2(0f, 0f, size.X, depth), new Color(0.11f, 0.15f, 0.25f));
        canvas.DrawRect(new Rect2(0f, size.Y - depth, size.X, depth), new Color(0.11f, 0.15f, 0.25f));
        canvas.DrawRect(new Rect2(0f, 0f, depth, size.Y), new Color(0.11f, 0.15f, 0.25f));
        canvas.DrawRect(new Rect2(size.X - depth, 0f, depth, size.Y), new Color(0.11f, 0.15f, 0.25f));

        for (var x = 0f; x < size.X; x += 48f)
        {
            canvas.DrawLine(new Vector2(x, 0f), new Vector2(x, depth), new Color(Cyan, 0.16f), 1f);
            canvas.DrawLine(new Vector2(x + 24f, size.Y - depth), new Vector2(x + 24f, size.Y), new Color(Ember, 0.18f), 1f);
        }
    }

    private static void DrawPixelCorners(CanvasItem canvas, Rect2 rect, Color color, float length)
    {
        var a = rect.Position;
        var b = new Vector2(rect.End.X, rect.Position.Y);
        var c = rect.End;
        var d = new Vector2(rect.Position.X, rect.End.Y);

        canvas.DrawLine(a, a + new Vector2(length, 0f), color, 3f);
        canvas.DrawLine(a, a + new Vector2(0f, length), color, 3f);
        canvas.DrawLine(b, b + new Vector2(-length, 0f), color, 3f);
        canvas.DrawLine(b, b + new Vector2(0f, length), color, 3f);
        canvas.DrawLine(c, c + new Vector2(-length, 0f), color, 3f);
        canvas.DrawLine(c, c + new Vector2(0f, -length), color, 3f);
        canvas.DrawLine(d, d + new Vector2(length, 0f), color, 3f);
        canvas.DrawLine(d, d + new Vector2(0f, -length), color, 3f);
    }
}
