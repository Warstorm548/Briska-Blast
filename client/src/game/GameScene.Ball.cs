using Godot;

namespace BriskaBlast.Game;

/// <summary>The master ball's local lifecycle: reporting a ball lost through this
/// screen's goal, and putting the next one on the paddle to be served.</summary>
public partial class GameScene
{
    private bool _awaitingServe;
    private Ball? _serveBall;

    /// <summary>A ball left through this screen's goal. Reports it to the server —
    /// which is authoritative for the tally; nothing is scored locally — and
    /// re-serves if the ball lost was the master.</summary>
    private void OnScore(ScoreEvent e)
    {
        // Report to the server (server-relayed scoring) — the controller drops
        // empty scorers (self-goal / untouched).
        _controller?.ReportScore(e);

        // Only a lost master ball is replaced: the scored-on player serves the next
        // one. A split (BallBT) ball is a bonus — it just vanishes, no re-serve.
        if (e.Kind == BallKind.Master)
            SpawnServeBall();
    }

    /// <summary>Put a fresh master ball on the paddle and wait for the serve. Split
    /// balls already in play are untouched.</summary>
    private void SpawnServeBall()
    {
        // Serve a fresh master ball. Any split balls in play are left alone — only
        // the lost master is replaced. Exactly one master exists at a time (it's the
        // single ball handed between screens), so there's nothing to clear here.
        _serveBall = new Ball
        {
            Id = _state.NextBallId(),
            Radius = _ballRadius,
            Kind = BallKind.Master,
            Pos = new Vector2(_state.Paddle.CenterX, _state.Paddle.Y - _ballRadius),
        };
        _state.Balls.Add(_serveBall);
        _awaitingServe = true;
    }
}
