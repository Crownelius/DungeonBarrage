using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// Slingshot drag: pull away from the opponent; line length is power.
/// </summary>
public sealed class AimSolverTests
{
    private const int Full = AimSolver.MaxPowerBasisPoints;

    [Fact]
    public void A_crow_on_the_left_must_drag_left_and_fires_right()
    {
        Assert.True(AimSolver.FacesRight(actorX: 100, opponentX: 800));
        var aim = AimSolver.FromDrag(200, 50, 80, 50, facesRight: true, Full);
        Assert.Equal(0, aim.AngleMillidegrees);
        Assert.True(aim.CanFire);
        Assert.True(aim.FacesRight);
    }

    [Fact]
    public void A_crow_on_the_right_must_drag_right_and_fires_left()
    {
        Assert.False(AimSolver.FacesRight(actorX: 800, opponentX: 100));
        var aim = AimSolver.FromDrag(80, 50, 200, 50, facesRight: false, Full);
        Assert.Equal(180_000, aim.AngleMillidegrees);
        Assert.True(aim.CanFire);
    }

    [Fact]
    public void Dragging_toward_the_opponent_does_not_fire()
    {
        var leftCrowDraggedRight = AimSolver.FromDrag(100, 100, 220, 100, facesRight: true, Full);
        var rightCrowDraggedLeft = AimSolver.FromDrag(200, 100, 80, 100, facesRight: false, Full);
        Assert.False(leftCrowDraggedRight.CanFire);
        Assert.False(rightCrowDraggedLeft.CanFire);
    }

    [Fact]
    public void A_purely_vertical_line_does_not_fire()
    {
        var up = AimSolver.FromDrag(100, 200, 100, 40, facesRight: true, Full);
        Assert.False(up.CanFire);
    }

    [Fact]
    public void Pulling_left_and_down_lobs_up_and_right()
    {
        var aim = AimSolver.FromDrag(0, 0, -100, 100, facesRight: true, Full);
        Assert.True(aim.CanFire);
        Assert.InRange(aim.AngleMillidegrees, 40_000, 50_000);
    }

    [Fact]
    public void Power_is_the_length_of_the_drawn_line_scaled_to_this_turns_maximum()
    {
        var halfCap = AimSolver.MaxPowerAfterMovement(8 * 1024, 16 * 1024);
        Assert.Equal(Full / 2, halfCap);

        var shortLine = AimSolver.FromDrag(0, 0, -AimSolver.FullPowerPixels / 2f, 0, facesRight: true, Full);
        var fullLine = AimSolver.FromDrag(0, 0, -AimSolver.FullPowerPixels, 0, facesRight: true, Full);
        var afterWalk = AimSolver.FromDrag(0, 0, -AimSolver.FullPowerPixels, 0, facesRight: true, halfCap);

        Assert.Equal(Full / 2, shortLine.PowerBasisPoints);
        Assert.Equal(Full, fullLine.PowerBasisPoints);
        Assert.Equal(halfCap, afterWalk.PowerBasisPoints);
        Assert.Equal(100, afterWalk.PowerPercent);
    }

    [Fact]
    public void A_full_move_still_leaves_a_ten_percent_floor()
    {
        var floor = AimSolver.MaxPowerAfterMovement(0, 16 * 1024);
        Assert.Equal(AimSolver.MinimumMaxPowerBasisPoints, floor);
    }

    [Fact]
    public void A_twitch_does_not_fire()
    {
        var aim = AimSolver.FromDrag(50, 50, 45, 50, facesRight: true, Full);
        Assert.False(aim.CanFire);
    }

    [Fact]
    public void A_twenty_cell_line_is_full_power_at_the_live_cell_size()
    {
        const float cell = 24f;
        var aim = AimSolver.FromDrag(0, 0, -20f * cell, 0, facesRight: true, Full, cell);
        Assert.Equal(Full, aim.PowerBasisPoints);
        Assert.True(aim.CanFire);
    }
}
