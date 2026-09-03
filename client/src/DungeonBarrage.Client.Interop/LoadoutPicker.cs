using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>Which loadout page the player is on.</summary>
public enum LoadoutStage
{
    /// <summary>Ranged main weapon.</summary>
    Main,

    /// <summary>Melee visual.</summary>
    Melee,

    /// <summary>Single-use secondary round.</summary>
    Secondary,

    /// <summary>Crown or anklet.</summary>
    Trinket,
}

/// <summary>
/// Sequential loadout wizard: ranged, then melee, then secondary, then trinket.
/// Clicking a tile on the current page equips that slot. ENTER advances; the last
/// ENTER is Confirm.
/// </summary>
public sealed class LoadoutPicker
{
    private readonly IReadOnlyList<ClientItemDefinition> _items;
    private int _mainIndex;
    private int _secondaryIndex;
    private int _meleeIndex;
    private int _trinketIndex;

    /// <summary>Creates a picker over a catalog, defaulting each slot to the launch loadout.</summary>
    /// <param name="items">Catalog items. Must contain at least one entry per slot.</param>
    public LoadoutPicker(IReadOnlyList<ClientItemDefinition> items)
    {
        ArgumentNullException.ThrowIfNull(items);
        if (items.Count == 0)
        {
            throw new ArgumentException("The item catalog must not be empty.", nameof(items));
        }

        _items = items;
        _mainIndex = IndexOfSlot(items, ClientAbilitySlot.Main, "ramshot-cannon");
        _secondaryIndex = IndexOfSlot(items, ClientAbilitySlot.Secondary, "ramshot-shell");
        _meleeIndex = IndexOfSlot(items, ClientAbilitySlot.MeleeTool, "trench-spade");
        _trinketIndex = IndexOfSlot(items, ClientAbilitySlot.Trinket, "ember-crown");
        Stage = LoadoutStage.Main;
        FocusedIndex = _mainIndex;
    }

    /// <summary>Catalog the picker was constructed with.</summary>
    public IReadOnlyList<ClientItemDefinition> Items => _items;

    /// <summary>Current wizard page.</summary>
    public LoadoutStage Stage { get; private set; }

    /// <summary>Tile currently focused by keyboard or last click (catalog index).</summary>
    public int FocusedIndex { get; private set; }

    /// <summary>Catalog index equipped in the main slot.</summary>
    public int MainIndex => _mainIndex;

    /// <summary>Catalog index equipped in the secondary slot.</summary>
    public int SecondaryIndex => _secondaryIndex;

    /// <summary>Catalog index equipped in the melee slot.</summary>
    public int MeleeIndex => _meleeIndex;

    /// <summary>Catalog index equipped in the trinket slot.</summary>
    public int TrinketIndex => _trinketIndex;

    /// <summary>The loadout Confirm will put on the create envelope.</summary>
    public ClientLoadout Loadout =>
        new(IdAt(_mainIndex), IdAt(_secondaryIndex), IdAt(_meleeIndex), IdAt(_trinketIndex));

    /// <summary>Whether this page is the last before the match starts.</summary>
    public bool IsLastStage => Stage == LoadoutStage.Trinket;

    /// <summary>Title for the current page.</summary>
    public string StageTitle => Stage switch
    {
        LoadoutStage.Main => "1 / 4  RANGED",
        LoadoutStage.Melee => "2 / 4  MELEE",
        LoadoutStage.Secondary => "3 / 4  SECONDARY  (one shot)",
        LoadoutStage.Trinket => "4 / 4  CROWN / ANKLET",
        _ => "LOADOUT",
    };

    /// <summary>Footer hint for the current page.</summary>
    public string StageHint => Stage switch
    {
        LoadoutStage.Main => "Click a weapon — gold tile is equipped · NEXT SLOT / ENTER continues to melee · ESC back",
        LoadoutStage.Melee => "Click a melee item (same strike, different look) · NEXT SLOT continues · ESC back",
        LoadoutStage.Secondary => "Click a one-shot round · NEXT SLOT continues to crown/anklet · ESC back",
        LoadoutStage.Trinket => "Click a crown or anklet · START DUEL / ENTER begins the match · ESC back",
        _ => "ENTER continues",
    };

    /// <summary>Catalog indices visible on the current page, in catalog order.</summary>
    /// <returns>Indices of items whose slot matches <see cref="Stage"/>.</returns>
    public IReadOnlyList<int> VisibleCatalogIndices()
    {
        var slot = SlotOf(Stage);
        var list = new List<int>();
        for (var i = 0; i < _items.Count; i++)
        {
            if (_items[i].Slot == slot)
            {
                list.Add(i);
            }
        }

        return list;
    }

    /// <summary>Equipped catalog index for the current page.</summary>
    public int EquippedIndexForStage => Stage switch
    {
        LoadoutStage.Main => _mainIndex,
        LoadoutStage.Melee => _meleeIndex,
        LoadoutStage.Secondary => _secondaryIndex,
        LoadoutStage.Trinket => _trinketIndex,
        _ => _mainIndex,
    };

    /// <summary>
    /// Focuses <paramref name="index"/> and, if that tile belongs to the current page,
    /// equips it into that slot.
    /// </summary>
    /// <param name="index">Catalog index of the clicked or keyboard-selected tile.</param>
    public void SelectTile(int index)
    {
        if (index < 0 || index >= _items.Count)
        {
            return;
        }

        if (_items[index].Slot != SlotOf(Stage))
        {
            return;
        }

        FocusedIndex = index;
        switch (_items[index].Slot)
        {
            case ClientAbilitySlot.Main:
                _mainIndex = index;
                break;
            case ClientAbilitySlot.Secondary:
                _secondaryIndex = index;
                break;
            case ClientAbilitySlot.MeleeTool:
                _meleeIndex = index;
                break;
            case ClientAbilitySlot.Trinket:
                _trinketIndex = index;
                break;
        }
    }

    /// <summary>Advances to the next page. Returns <see langword="false"/> on the last page.</summary>
    /// <returns><see langword="true"/> if a later page exists.</returns>
    public bool TryAdvance()
    {
        if (IsLastStage)
        {
            return false;
        }

        Stage = Stage switch
        {
            LoadoutStage.Main => LoadoutStage.Melee,
            LoadoutStage.Melee => LoadoutStage.Secondary,
            LoadoutStage.Secondary => LoadoutStage.Trinket,
            _ => Stage,
        };
        FocusedIndex = EquippedIndexForStage;
        return true;
    }

    /// <summary>Returns to the previous page. Returns <see langword="false"/> on the first page.</summary>
    /// <returns><see langword="true"/> if a prior page exists.</returns>
    public bool TryRetreat()
    {
        if (Stage == LoadoutStage.Main)
        {
            return false;
        }

        Stage = Stage switch
        {
            LoadoutStage.Melee => LoadoutStage.Main,
            LoadoutStage.Secondary => LoadoutStage.Melee,
            LoadoutStage.Trinket => LoadoutStage.Secondary,
            _ => Stage,
        };
        FocusedIndex = EquippedIndexForStage;
        return true;
    }

    /// <summary>Whether <paramref name="index"/> is the equipped tile on the current page.</summary>
    /// <param name="index">Catalog index.</param>
    /// <returns><see langword="true"/> if that tile is currently equipped for this page.</returns>
    public bool IsEquipped(int index) => index == EquippedIndexForStage;

    private string IdAt(int index) =>
        index >= 0 && index < _items.Count ? _items[index].Id : string.Empty;

    private static ClientAbilitySlot SlotOf(LoadoutStage stage) => stage switch
    {
        LoadoutStage.Main => ClientAbilitySlot.Main,
        LoadoutStage.Melee => ClientAbilitySlot.MeleeTool,
        LoadoutStage.Secondary => ClientAbilitySlot.Secondary,
        LoadoutStage.Trinket => ClientAbilitySlot.Trinket,
        _ => ClientAbilitySlot.Main,
    };

    private static int IndexOfSlot(
        IReadOnlyList<ClientItemDefinition> items,
        ClientAbilitySlot slot,
        string preferredId)
    {
        for (var i = 0; i < items.Count; i++)
        {
            if (items[i].Slot == slot && items[i].Id == preferredId)
            {
                return i;
            }
        }

        for (var i = 0; i < items.Count; i++)
        {
            if (items[i].Slot == slot)
            {
                return i;
            }
        }

        throw new ArgumentException(
            $"The item catalog has no entry for the {slot} slot.", nameof(items));
    }
}
