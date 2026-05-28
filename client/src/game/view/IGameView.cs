using BriskaBlast.Game;

namespace BriskaBlast.Game.View;

/// <summary>
/// Draws a <see cref="GameState"/>. The view only ever observes state — it never
/// owns or mutates it — which is what keeps "2D now, 3D later" a swap rather than
/// a rewrite: a future <c>View3D : Node3D</c> implements the same contract and
/// maps the same (x, y) data to Vector3. See the Stage 3 plan.
/// </summary>
public interface IGameView
{
    /// <summary>Reflect the current state on screen. Called once per frame after
    /// the simulation step.</summary>
    void Render(GameState state);
}
