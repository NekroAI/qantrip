using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Hosting;
using Qantrip.Platform;

namespace Qantrip.UI;

/// <summary>
/// 面板动效（D15，对应 macOS LauncherView / LauncherPanel 的 SwiftUI 动画）：入场弹性缩放、候选淡入滑入、回车被阻止时抖动、图标弹跳。
/// 全部用 Composition 动画在合成线程上运行，不阻塞输入；系统关闭"动画效果"时不播放（Motion.Enabled）。
/// </summary>
internal static class Anim
{
    private static Visual V(UIElement e) => ElementCompositionPreview.GetElementVisual(e);

    private static CompositionEasingFunction EaseOut(Compositor c) =>
        c.CreateCubicBezierEasingFunction(new Vector2(0.2f, 0.8f), new Vector2(0.2f, 1f));

    private static void Center(Visual v, FrameworkElement e) =>
        v.CenterPoint = new Vector3((float)(e.ActualWidth / 2), (float)(e.ActualHeight / 2), 0);

    /// 淡入并从 `from` 位移处滑回原位。
    public static void FadeSlideIn(UIElement e, Vector3 from, double ms, double delayMs = 0)
    {
        if (!Motion.Enabled) return;
        ElementCompositionPreview.SetIsTranslationEnabled(e, true);
        var v = V(e);
        var c = v.Compositor;
        var op = c.CreateScalarKeyFrameAnimation();
        op.InsertKeyFrame(0, 0);
        op.InsertKeyFrame(1, 1, EaseOut(c));
        var tr = c.CreateVector3KeyFrameAnimation();
        tr.InsertKeyFrame(0, from);
        tr.InsertKeyFrame(1, Vector3.Zero, EaseOut(c));
        foreach (var a in new KeyFrameAnimation[] { op, tr })
        {
            a.Duration = TimeSpan.FromMilliseconds(ms);
            a.DelayTime = TimeSpan.FromMilliseconds(delayMs);
            a.DelayBehavior = AnimationDelayBehavior.SetInitialValueBeforeDelay;
        }
        v.StartAnimation("Opacity", op);
        v.StartAnimation("Translation", tr);
    }

    /// 面板入场：内容从 96% 弹性放大并淡入（约 200ms）。
    public static void Entrance(FrameworkElement e)
    {
        if (!Motion.Enabled) return;
        var v = V(e);
        var c = v.Compositor;
        Center(v, e);
        var scale = c.CreateSpringVector3Animation();
        scale.InitialValue = new Vector3(0.96f, 0.96f, 1);
        scale.FinalValue = Vector3.One;
        scale.DampingRatio = 0.72f;
        scale.Period = TimeSpan.FromMilliseconds(42);
        var op = c.CreateScalarKeyFrameAnimation();
        op.InsertKeyFrame(0, 0);
        op.InsertKeyFrame(1, 1, EaseOut(c));
        op.Duration = TimeSpan.FromMilliseconds(140);
        v.StartAnimation("Scale", scale);
        v.StartAnimation("Opacity", op);
    }

    /// 回车被阻止：水平抖动（与 macOS 相同的关键帧，320ms）。
    public static void Shake(UIElement e)
    {
        if (!Motion.Enabled) return;
        ElementCompositionPreview.SetIsTranslationEnabled(e, true);
        var v = V(e);
        var c = v.Compositor;
        var a = c.CreateScalarKeyFrameAnimation();
        float[] xs = { 0, -9, 8, -6, 4, -2, 0 };
        for (int i = 0; i < xs.Length; i++) a.InsertKeyFrame(i / (float)(xs.Length - 1), xs[i]);
        a.Duration = TimeSpan.FromMilliseconds(320);
        v.StartAnimation("Translation.X", a);
    }

    /// 图标弹跳（命令锁定时）。
    public static void Pop(FrameworkElement e, float peak = 1.25f)
    {
        if (!Motion.Enabled) return;
        var v = V(e);
        var c = v.Compositor;
        Center(v, e);
        var a = c.CreateVector3KeyFrameAnimation();
        a.InsertKeyFrame(0.35f, new Vector3(peak, peak, 1), EaseOut(c));
        a.InsertKeyFrame(1, Vector3.One, EaseOut(c));
        a.Duration = TimeSpan.FromMilliseconds(260);
        v.StartAnimation("Scale", a);
    }
}
